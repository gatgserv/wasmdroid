//! WASIX adapter over engine capabilities, never over ambient host paths.
use std::{
    fmt,
    io::{Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncSeek, AsyncWrite, ReadBuf};
use wasmdroid_engine::volumes::{VolumeDirectory, VolumeFile, VolumeMetadata, VolumeOpenOptions};
use wasmer_wasix::virtual_fs::{self as vfs, FileOpener, FileSystem, FsError, VirtualFile};

#[derive(Clone)]
pub(crate) struct VolumeFs(pub VolumeDirectory);
impl fmt::Debug for VolumeFs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VolumeFs")
            .field("read_only", &self.0.read_only())
            .finish()
    }
}
fn relative(path: &Path) -> vfs::Result<PathBuf> {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(part) => result.push(part),
            _ => return Err(FsError::PermissionDenied),
        }
    }
    if result.as_os_str().is_empty() {
        result.push(".");
    }
    Ok(result)
}
fn convert(value: VolumeMetadata) -> vfs::Metadata {
    vfs::Metadata {
        ft: vfs::FileType {
            dir: value.is_dir,
            file: value.is_file,
            symlink: value.is_symlink,
            ..Default::default()
        },
        accessed: value.accessed_ns,
        created: value.created_ns,
        modified: value.modified_ns,
        len: value.len,
    }
}
impl FileSystem for VolumeFs {
    fn readlink(&self, _: &Path) -> vfs::Result<PathBuf> {
        Err(FsError::Unsupported)
    }
    fn read_dir(&self, path: &Path) -> vfs::Result<vfs::ReadDir> {
        let relative = relative(path)?;
        let entries = self
            .0
            .read_dir(relative)?
            .into_iter()
            .map(|entry| vfs::DirEntry {
                path: path.join(entry.name),
                metadata: Ok(convert(entry.metadata)),
            })
            .collect();
        Ok(vfs::ReadDir::new(entries))
    }
    fn create_dir(&self, path: &Path) -> vfs::Result<()> {
        Ok(self.0.create_dir(relative(path)?)?)
    }
    fn remove_dir(&self, path: &Path) -> vfs::Result<()> {
        Ok(self.0.remove_dir(relative(path)?)?)
    }
    fn rename<'a>(
        &'a self,
        from: &'a Path,
        to: &'a Path,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = vfs::Result<()>> + Send + 'a>> {
        Box::pin(async move { Ok(self.0.rename(relative(from)?, relative(to)?)?) })
    }
    fn metadata(&self, path: &Path) -> vfs::Result<vfs::Metadata> {
        Ok(convert(self.0.metadata(relative(path)?)?))
    }
    fn symlink_metadata(&self, path: &Path) -> vfs::Result<vfs::Metadata> {
        Ok(convert(self.0.symlink_metadata(relative(path)?)?))
    }
    fn remove_file(&self, path: &Path) -> vfs::Result<()> {
        Ok(self.0.remove_file(relative(path)?)?)
    }
    fn new_open_options(&self) -> vfs::OpenOptions<'_> {
        vfs::OpenOptions::new(self)
    }
}
impl FileOpener for VolumeFs {
    fn open(
        &self,
        path: &Path,
        conf: &vfs::OpenOptionsConfig,
    ) -> vfs::Result<Box<dyn VirtualFile + Send + Sync>> {
        let path = relative(path)?;
        let file = self.0.open_file(
            &path,
            VolumeOpenOptions {
                read: conf.read,
                write: conf.write,
                create: conf.create,
                create_new: conf.create_new,
                truncate: conf.truncate,
                append: conf.append,
            },
        )?;
        Ok(Box::new(GuestFile {
            file,
            directory: self.0.clone(),
            path,
            seek_result: None,
        }))
    }
}
struct GuestFile {
    file: VolumeFile,
    directory: VolumeDirectory,
    path: PathBuf,
    seek_result: Option<std::io::Result<u64>>,
}
impl fmt::Debug for GuestFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VolumeFile").finish_non_exhaustive()
    }
}
impl AsyncRead for GuestFile {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let count = self.file.read(buf.initialize_unfilled())?;
        buf.advance(count);
        Poll::Ready(Ok(()))
    }
}
impl AsyncWrite for GuestFile {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Poll::Ready(self.file.write(buf))
    }
    fn poll_flush(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(self.file.flush().and_then(|()| self.file.sync_all()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        self.poll_flush(cx)
    }
}
impl AsyncSeek for GuestFile {
    fn start_seek(mut self: Pin<&mut Self>, position: SeekFrom) -> std::io::Result<()> {
        self.seek_result = Some(self.file.seek(position));
        Ok(())
    }
    fn poll_complete(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<u64>> {
        Poll::Ready(
            self.seek_result
                .take()
                .unwrap_or_else(|| self.file.stream_position()),
        )
    }
}
fn nanos(time: std::io::Result<std::time::SystemTime>) -> u64 {
    time.ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|t| t.as_nanos().try_into().ok())
        .unwrap_or(0)
}
impl VirtualFile for GuestFile {
    fn last_accessed(&self) -> u64 {
        self.file
            .metadata()
            .map(|m| nanos(m.accessed()))
            .unwrap_or(0)
    }
    fn last_modified(&self) -> u64 {
        self.file
            .metadata()
            .map(|m| nanos(m.modified()))
            .unwrap_or(0)
    }
    fn created_time(&self) -> u64 {
        self.file
            .metadata()
            .map(|m| nanos(m.created()))
            .unwrap_or(0)
    }
    fn set_times(&mut self, _: Option<u64>, _: Option<u64>) -> vfs::Result<()> {
        Err(FsError::Unsupported)
    }
    fn size(&self) -> u64 {
        self.file.metadata().map(|m| m.len()).unwrap_or(0)
    }
    fn set_len(&mut self, len: u64) -> vfs::Result<()> {
        Ok(self.file.set_len(len)?)
    }
    fn unlink(&mut self) -> vfs::Result<()> {
        Ok(self.directory.remove_file(&self.path)?)
    }
    fn poll_read_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<usize>> {
        Poll::Ready(Ok(self.size().min(usize::MAX as u64) as usize))
    }
    fn poll_write_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<usize>> {
        Poll::Ready(if self.directory.read_only() {
            Err(std::io::ErrorKind::PermissionDenied.into())
        } else {
            Ok(8192)
        })
    }
}
