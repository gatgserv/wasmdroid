use wasmdroid_engine::{
    EngineError, Result,
    api::{InstanceSpec, Protocol},
};
use webc::metadata::annotations::Wasi;

const LABEL: &str = "dev.wasmdroid.startup.port";

pub(crate) fn configured_port(spec: &InstanceSpec) -> Result<Option<u16>> {
    let Some(value) = spec.labels.get(LABEL) else {
        return Ok(None);
    };
    let port = decimal_port(value)
        .filter(|port| *port != 0 && port.to_string() == *value)
        .ok_or_else(|| {
            EngineError::Invalid("startup port must be an integer from 1 to 65535".into())
        })?;
    if !spec
        .ports
        .iter()
        .any(|binding| binding.protocol == Protocol::Tcp && binding.instance_port == port)
    {
        return Err(EngineError::Invalid(
            "startup port must match a published application TCP port".into(),
        ));
    }
    Ok(Some(port))
}

pub(crate) fn runtime_arguments(spec: &InstanceSpec, wasi: &mut Wasi) -> Result<Vec<String>> {
    let Some(port) = configured_port(spec)? else {
        return Ok(spec.args.clone());
    };
    // The runner normally appends the configured arguments after the manifest
    // arguments. Assemble the same stream so the guest receives one --port option.
    let manifest = wasi.main_args.take().unwrap_or_default();
    let mut args = Vec::with_capacity(manifest.len() + spec.args.len() + 2);
    let mut tokens = manifest.into_iter().chain(spec.args.iter().cloned());
    let mut replacement = Some(["--port".into(), port.to_string()]);
    while let Some(token) = tokens.next() {
        if token == "--" {
            args.extend(replacement.take().into_iter().flatten());
            args.push(token);
            args.extend(tokens);
            return Ok(args);
        }

        let value = if token == "--port" {
            tokens.next()
        } else if let Some(value) = token.strip_prefix("--port=") {
            Some(value.to_owned())
        } else {
            args.push(token);
            continue;
        };
        if value.as_deref().and_then(decimal_port).is_none() {
            return Err(EngineError::Invalid(
                "an existing --port argument needs a decimal TCP port".into(),
            ));
        }
        args.extend(replacement.take().into_iter().flatten());
    }
    args.extend(replacement.into_iter().flatten());
    Ok(args)
}

fn decimal_port(value: &str) -> Option<u16> {
    if value.bytes().all(|byte| byte.is_ascii_digit()) {
        value.parse().ok()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn spec(port: &str, args: &[&str]) -> InstanceSpec {
        serde_json::from_value(json!({
            "artifact_ref": "wasmer.io/carlosriojas/blogbase@0.3.6",
            "command": "script",
            "args": args,
            "ports": [{ "host_port": 0, "instance_port": 8082, "protocol": "tcp" }],
            "labels": { (LABEL): port }
        }))
        .unwrap()
    }

    fn annotation(args: &[&str]) -> Wasi {
        let mut wasi = Wasi::new("script");
        wasi.main_args = Some(args.iter().map(|arg| (*arg).into()).collect());
        wasi
    }

    #[test]
    fn port_replaces_both_manifest_and_custom_options() {
        let spec = spec("8082", &["--port=6000", "--listen", "::1"]);
        let mut wasi = annotation(&["--root", "/public", "--port", "8080"]);
        assert_eq!(
            runtime_arguments(&spec, &mut wasi).unwrap(),
            ["--root", "/public", "--port", "8082", "--listen", "::1"]
        );
        assert!(wasi.main_args.is_none());
    }

    #[test]
    fn port_is_inserted_before_terminator_and_positional_arguments_are_preserved() {
        let spec = spec("8082", &["--", "--port=6000"]);
        let mut wasi = annotation(&["--root=/public"]);
        assert_eq!(
            runtime_arguments(&spec, &mut wasi).unwrap(),
            ["--root=/public", "--port", "8082", "--", "--port=6000"]
        );
    }

    #[test]
    fn port_is_appended_when_the_package_has_no_port_argument() {
        let spec = spec("8082", &["--log-level=info"]);
        let mut wasi = Wasi::new("script");
        assert_eq!(
            runtime_arguments(&spec, &mut wasi).unwrap(),
            ["--log-level=info", "--port", "8082"]
        );
    }

    #[test]
    fn port_requires_a_matching_published_tcp_port() {
        for label in ["", "0", "08082", "+8082", "8082 ", "8083", "65536"] {
            assert!(configured_port(&spec(label, &[])).is_err());
        }
        let mut spec = spec("8082", &[]);
        assert_eq!(configured_port(&spec).unwrap(), Some(8082));
        spec.ports[0].protocol = Protocol::Udp;
        assert!(configured_port(&spec).is_err());
    }

    #[test]
    fn malformed_existing_port_is_reported_only_with_explicit_opt_in() {
        let mut spec = spec("8082", &["--port", "--debug"]);
        let mut wasi = annotation(&["--port=8080"]);
        assert!(runtime_arguments(&spec, &mut wasi).is_err());
        spec.labels.remove(LABEL);
        let mut wasi = annotation(&["--port=8080"]);
        assert_eq!(runtime_arguments(&spec, &mut wasi).unwrap(), spec.args);
        assert_eq!(wasi.main_args.unwrap(), ["--port=8080"]);
    }
}
