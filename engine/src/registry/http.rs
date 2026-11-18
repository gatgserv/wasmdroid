//! Synchronous registry facade over cancellable async request/body futures.
//! One bounded process runtime; no per-request detached blocking HTTP threads.
use crate::{EngineError, Result, operation::OperationControl};
use std::{
    future::Future,
    io::Read,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

// getaddrinfo cannot be forcibly interrupted safely. Keep a slot inside the
// queued/running blocking job even if its caller is cancelled, so repeated
// cancellation cannot create an unbounded resolver job backlog.
static DNS_JOBS: AtomicUsize = AtomicUsize::new(0);
struct DnsSlot;
impl Drop for DnsSlot {
    fn drop(&mut self) {
        DNS_JOBS.fetch_sub(1, Ordering::AcqRel);
    }
}
struct Resolver;
impl reqwest::dns::Resolve for Resolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        Box::pin(async move {
            DNS_JOBS
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                    (count < 4).then_some(count + 1)
                })
                .map_err(|_| std::io::Error::other("DNS resolver is busy"))?;
            let slot = DnsSlot;
            let result = tokio::task::spawn_blocking(move || {
                use std::net::ToSocketAddrs;
                let _slot = slot;
                (name.as_str(), 0)
                    .to_socket_addrs()
                    .map(|addresses| Box::new(addresses.take(64)) as reqwest::dns::Addrs)
                    .map_err(|_| std::io::Error::other("DNS resolution failed"))
            })
            .await
            .map_err(|_| std::io::Error::other("DNS resolver unavailable"))?;
            Ok(result?)
        })
    }
}

pub(crate) struct Http {
    client: reqwest::Client,
    control: Arc<OperationControl>,
}
fn runtime() -> Result<&'static tokio::runtime::Runtime> {
    static RUNTIME: OnceLock<std::io::Result<tokio::runtime::Runtime>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .max_blocking_threads(2)
                .thread_name("wasmdroid-http")
                .enable_all()
                .build()
        })
        .as_ref()
        .map_err(|_| EngineError::Network("HTTP executor unavailable".into()))
}
pub(super) fn wait<T>(
    control: &OperationControl,
    future: impl Future<Output = reqwest::Result<T>>,
) -> Result<T> {
    control.check()?;
    runtime()?.block_on(async{
        tokio::select!{biased;
            _=control.cancelled()=>Err(EngineError::Cancelled),
            result=future=>result.map_err(|error|EngineError::Network(error.without_url().to_string())),
        }
    })
}
impl Http {
    /// Bounded curated metadata GET; never authenticates or logs source URLs.
    pub fn get_bounded(&self, url: &str, max: u64) -> Result<Vec<u8>> {
        super::wasmer::validate_download_url(url)?;
        let mut response = self.send(self.client.get(url))?;
        super::wasmer::validate_download_url(response.url().as_str())?;
        if response.content_length().is_some_and(|length| length > max) {
            return Err(EngineError::Invalid(
                "metadata response exceeds limit".into(),
            ));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = wait(&self.control, response.chunk())? {
            if (bytes.len() as u64).saturating_add(chunk.len() as u64) > max {
                return Err(EngineError::Invalid(
                    "metadata response exceeds limit".into(),
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        self.control.check()?;
        Ok(bytes)
    }
    pub fn new(control: Arc<OperationControl>) -> Result<Self> {
        runtime()?;
        let client = reqwest::Client::builder()
            .https_only(true)
            .dns_resolver(Arc::new(Resolver))
            .user_agent(concat!("WasmDroid/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(180))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 5 {
                    attempt.error("redirect limit exceeded")
                } else if super::wasmer::validate_download_url(attempt.url().as_str()).is_err() {
                    attempt.error("unsafe redirect target")
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .map_err(|error| EngineError::Network(error.without_url().to_string()))?;
        Ok(Self { client, control })
    }
    fn send(&self, request: reqwest::RequestBuilder) -> Result<reqwest::Response> {
        // RequestBuilder::send creates the request timeout eagerly. Construct
        // the future only while polling inside the process HTTP runtime.
        let response = wait(&self.control, async { request.send().await })?;
        self.control.check()?;
        response
            .error_for_status()
            .map_err(|error| EngineError::Network(error.without_url().to_string()))
    }
    pub fn post_json(&self, url: &str, value: &serde_json::Value, max: u64) -> Result<Vec<u8>> {
        let mut response = self.send(self.client.post(url).json(value))?;
        let mut bytes = Vec::new();
        while let Some(chunk) = wait(&self.control, response.chunk())? {
            if (bytes.len() as u64).saturating_add(chunk.len() as u64) > max {
                return Err(EngineError::Registry(
                    "metadata response exceeds limit".into(),
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        self.control.check()?;
        Ok(bytes)
    }
    pub fn download(&self, url: &str, size: u64) -> Result<Box<dyn Read + Send>> {
        let response = self.send(self.client.get(url))?;
        super::wasmer::validate_download_url(response.url().as_str())?;
        if response
            .content_length()
            .is_some_and(|length| length != size)
        {
            return Err(EngineError::Registry(
                "download size differs from registry metadata".into(),
            ));
        }
        Ok(Box::new(Body {
            response,
            control: self.control.clone(),
            pending: Vec::new(),
            position: 0,
            total: 0,
            limit: size,
        }))
    }
}
struct Body {
    response: reqwest::Response,
    control: Arc<OperationControl>,
    pending: Vec<u8>,
    position: usize,
    total: u64,
    limit: u64,
}
impl Read for Body {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        self.control.check().map_err(std::io::Error::other)?;
        while self.position == self.pending.len() {
            self.pending.clear();
            self.position = 0;
            let Some(chunk) =
                wait(&self.control, self.response.chunk()).map_err(std::io::Error::other)?
            else {
                return Ok(0);
            };
            self.total = self
                .total
                .checked_add(chunk.len() as u64)
                .ok_or_else(|| std::io::Error::other("download too large"))?;
            if self.total > self.limit {
                return Err(std::io::Error::other("download exceeds declared size"));
            }
            self.pending.extend_from_slice(&chunk);
            // Empty body frames are not EOF; avoid unbounded recursion.
        }
        let count = buffer.len().min(self.pending.len() - self.position);
        buffer[..count].copy_from_slice(&self.pending[self.position..self.position + count]);
        self.position += count;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_registry_post_from_sync_thread_returns_cancelled() {
        let control = Arc::new(OperationControl::default());
        let http = Http::new(control.clone()).unwrap();
        assert!(control.cancel());
        let result = http.post_json(
            super::super::wasmer::REGISTRY_ENDPOINT,
            &serde_json::json!({}),
            1024,
        );
        assert!(matches!(result, Err(EngineError::Cancelled)));
    }

    #[test]
    fn cancellation_drops_a_pending_tls_exchange_before_request_timeout() {
        // Transport-only loopback fixture: TLS never completes, no certificate
        // validation or production public-URL policy is disabled.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let (accepted, waiting) = std::sync::mpsc::channel();
        let (finish, done) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                match listener.accept() {
                    Ok((socket, _)) => {
                        let _ = accepted.send(());
                        let _ = done.recv_timeout(Duration::from_secs(5));
                        drop(socket);
                        break;
                    }
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && std::time::Instant::now() < deadline =>
                    {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    _ => break,
                }
            }
        });
        let control = Arc::new(OperationControl::default());
        let other = control.clone();
        let cancelling = std::thread::spawn(move || {
            let connected = waiting.recv_timeout(Duration::from_secs(3)).is_ok();
            other.cancel();
            connected
        });
        let mut http = Http::new(control).unwrap();
        // The hermetic loopback fixture must not depend on workstation proxy
        // environment. TLS verification remains enabled.
        http.client = reqwest::Client::builder()
            .https_only(true)
            .no_proxy()
            .timeout(Duration::from_secs(180))
            .build()
            .unwrap();
        let started = std::time::Instant::now();
        let result = http.post_json(
            &format!("https://127.0.0.1:{port}/"),
            &serde_json::json!({}),
            1024,
        );
        let elapsed = started.elapsed();
        let connected = cancelling.join().unwrap();
        let _ = finish.send(());
        server.join().unwrap();
        assert!(connected);
        assert!(matches!(result, Err(EngineError::Cancelled)));
        assert!(elapsed < Duration::from_secs(3));
    }
}
