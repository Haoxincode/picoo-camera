//! ACL-scoped local Windows control transport for native VCam surfaces.
//!
//! The pipe carries bounded control frames only; it never carries pixels. The
//! channel state machine in `native_surface` remains responsible for protocol
//! generations and offer/ack ordering.

use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::time::{Duration, Instant};

use thiserror::Error;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{LocalFree, ERROR_PIPE_CONNECTED, HANDLE, HLOCAL};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, IsValidSid, TokenUser,
    PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_FIRST_PIPE_INSTANCE,
    FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_SHARE_NONE, OPEN_EXISTING, PIPE_ACCESS_DUPLEX,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId,
    GetNamedPipeServerProcessId, PeekNamedPipe, WaitNamedPipeW, NAMED_PIPE_MODE,
    PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows::Win32::System::SystemServices::SECURITY_LOCAL_SERVICE_RID;
use windows::Win32::System::Threading::{
    OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};

pub const WINDOWS_NATIVE_PIPE_NAME: &str = r"\\.\pipe\PicooCamera.NativeVcam";
pub const WINDOWS_NATIVE_PIPE_MAX_FRAME: usize = 4096;

#[derive(Debug, Error)]
pub enum WindowsNativePipeError {
    #[error("Windows native pipe API failed: {0}")]
    Platform(#[from] windows::core::Error),
    #[error("native pipe frame exceeds {WINDOWS_NATIVE_PIPE_MAX_FRAME} bytes")]
    FrameTooLarge,
    #[error("native pipe peer closed")]
    Closed,
    #[error("native pipe peer process was not accepted")]
    PeerRejected,
    #[error("native pipe read timed out")]
    Timeout,
    #[error("native pipe I/O failed: {0}")]
    Io(#[from] io::Error),
}

fn pipe_name_wide() -> Vec<u16> {
    WINDOWS_NATIVE_PIPE_NAME
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect()
}

fn write_all(handle: HANDLE, mut bytes: &[u8]) -> Result<(), WindowsNativePipeError> {
    while !bytes.is_empty() {
        let mut written = 0u32;
        unsafe { WriteFile(handle, Some(bytes), Some(&mut written), None)? };
        if written == 0 {
            return Err(WindowsNativePipeError::Closed);
        }
        bytes = &bytes[written as usize..];
    }
    Ok(())
}

fn read_exact(handle: HANDLE, bytes: &mut [u8]) -> Result<(), WindowsNativePipeError> {
    let mut offset = 0;
    while offset < bytes.len() {
        let mut read = 0u32;
        unsafe { ReadFile(handle, Some(&mut bytes[offset..]), Some(&mut read), None)? };
        if read == 0 {
            return Err(WindowsNativePipeError::Closed);
        }
        offset += read as usize;
    }
    Ok(())
}

fn write_frame(handle: HANDLE, payload: &[u8]) -> Result<(), WindowsNativePipeError> {
    if payload.len() > WINDOWS_NATIVE_PIPE_MAX_FRAME {
        return Err(WindowsNativePipeError::FrameTooLarge);
    }
    let length = u32::try_from(payload.len()).map_err(|_| WindowsNativePipeError::FrameTooLarge)?;
    write_all(handle, &length.to_le_bytes())?;
    write_all(handle, payload)
}

fn read_frame(handle: HANDLE) -> Result<Vec<u8>, WindowsNativePipeError> {
    let mut length = [0u8; 4];
    read_exact(handle, &mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length > WINDOWS_NATIVE_PIPE_MAX_FRAME {
        return Err(WindowsNativePipeError::FrameTooLarge);
    }
    let mut payload = vec![0u8; length];
    read_exact(handle, &mut payload)?;
    Ok(payload)
}

unsafe fn secure_attributes(
) -> Result<(SECURITY_ATTRIBUTES, PSECURITY_DESCRIPTOR), WindowsNativePipeError> {
    // Local Service (LS) is the Frame Server identity. Keep the ACE limited to
    // the read/write rights needed by the byte-stream transport; callers must
    // still bind the accepted peer to an expected PID before sending frames.
    // There is deliberately no Everyone/anonymous ACE.
    let sddl = "D:P(A;;GRGW;;;LS)\0";
    let wide = sddl.encode_utf16().collect::<Vec<_>>();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    ConvertStringSecurityDescriptorToSecurityDescriptorW(
        PCWSTR(wide.as_ptr()),
        SDDL_REVISION_1,
        &mut descriptor,
        None,
    )?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: false.into(),
    };
    Ok((attributes, descriptor))
}

pub struct WindowsNativePipeServer {
    handle: OwnedHandle,
}

impl WindowsNativePipeServer {
    pub fn create() -> Result<Self, WindowsNativePipeError> {
        let name = pipe_name_wide();
        let (attributes, descriptor) = unsafe { secure_attributes()? };
        let handle = unsafe {
            CreateNamedPipeW(
                PCWSTR(name.as_ptr()),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                NAMED_PIPE_MODE(
                    PIPE_TYPE_BYTE.0
                        | PIPE_READMODE_BYTE.0
                        | PIPE_WAIT.0
                        | PIPE_REJECT_REMOTE_CLIENTS.0,
                ),
                1,
                WINDOWS_NATIVE_PIPE_MAX_FRAME as u32 + 4,
                WINDOWS_NATIVE_PIPE_MAX_FRAME as u32 + 4,
                0,
                Some(&attributes),
            )
        };
        unsafe {
            let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        }
        if handle.is_invalid() {
            return Err(windows::core::Error::from_thread().into());
        }
        let handle = unsafe { OwnedHandle::from_raw_handle(handle.0 as *mut _) };
        Ok(Self { handle })
    }

    /// Accept exactly one client and require its process ID to match the
    /// caller's allow-list. The PID check is mandatory at this boundary so a
    /// caller cannot accidentally start the native protocol on an arbitrary
    /// process that can open the ACL-scoped pipe.
    pub fn accept_for_pid(&self, expected_process_id: u32) -> Result<(), WindowsNativePipeError> {
        if expected_process_id == 0 {
            return Err(WindowsNativePipeError::PeerRejected);
        }
        match unsafe { ConnectNamedPipe(HANDLE(self.handle.as_raw_handle()), None) } {
            Ok(()) => {}
            Err(error) if error.code() == ERROR_PIPE_CONNECTED.to_hresult() => {}
            Err(error) => return Err(error.into()),
        }
        let process_id = match self.client_process_id() {
            Ok(process_id) => process_id,
            Err(error) => {
                self.disconnect_client();
                return Err(error);
            }
        };
        if process_id != expected_process_id {
            self.disconnect_client();
            return Err(WindowsNativePipeError::PeerRejected);
        }
        Ok(())
    }

    /// Accept a client after an explicit caller-supplied identity check.
    ///
    /// This is useful when the receiver validates a token or executable image
    /// rather than pinning one PID. The validator runs only after the pipe has
    /// connected and receives the OS-reported client PID.
    pub fn accept_with_peer_validator(
        &self,
        validate: impl FnOnce(u32) -> bool,
    ) -> Result<u32, WindowsNativePipeError> {
        match unsafe { ConnectNamedPipe(HANDLE(self.handle.as_raw_handle()), None) } {
            Ok(()) => {}
            Err(error) if error.code() == ERROR_PIPE_CONNECTED.to_hresult() => {}
            Err(error) => return Err(error.into()),
        }
        let process_id = match self.client_process_id() {
            Ok(process_id) => process_id,
            Err(error) => {
                self.disconnect_client();
                return Err(error);
            }
        };
        if !validate(process_id) {
            self.disconnect_client();
            return Err(WindowsNativePipeError::PeerRejected);
        }
        Ok(process_id)
    }

    /// Accept only a Frame Server process running as Windows Local Service.
    /// This is the production validator for the fixed pipe name; callers that
    /// use a test or broker identity must use the explicit validator API.
    pub fn accept_local_service(&self) -> Result<u32, WindowsNativePipeError> {
        self.accept_with_peer_validator(is_local_service_process)
    }

    pub fn disconnect_client(&self) {
        unsafe {
            let _ = DisconnectNamedPipe(HANDLE(self.handle.as_raw_handle()));
        }
    }

    pub fn client_process_id(&self) -> Result<u32, WindowsNativePipeError> {
        let mut process_id = 0u32;
        unsafe {
            GetNamedPipeClientProcessId(HANDLE(self.handle.as_raw_handle()), &mut process_id)?;
        }
        Ok(process_id)
    }

    pub fn read_frame(&self) -> Result<Vec<u8>, WindowsNativePipeError> {
        read_frame(HANDLE(self.handle.as_raw_handle()))
    }

    pub fn read_frame_timeout(&self, timeout: Duration) -> Result<Vec<u8>, WindowsNativePipeError> {
        read_frame_timeout(HANDLE(self.handle.as_raw_handle()), timeout)
    }

    pub fn write_frame(&self, payload: &[u8]) -> Result<(), WindowsNativePipeError> {
        write_frame(HANDLE(self.handle.as_raw_handle()), payload)
    }
}

fn read_frame_timeout(
    handle: HANDLE,
    timeout: Duration,
) -> Result<Vec<u8>, WindowsNativePipeError> {
    let deadline = Instant::now() + timeout;
    wait_for_bytes(handle, 4, deadline)?;
    let mut length = [0u8; 4];
    read_exact(handle, &mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length > WINDOWS_NATIVE_PIPE_MAX_FRAME {
        return Err(WindowsNativePipeError::FrameTooLarge);
    }
    let mut payload = vec![0u8; length];
    wait_for_bytes(handle, length, deadline)?;
    read_exact(handle, &mut payload)?;
    Ok(payload)
}

fn wait_for_bytes(
    handle: HANDLE,
    required: usize,
    deadline: Instant,
) -> Result<(), WindowsNativePipeError> {
    if required == 0 {
        return Ok(());
    }
    loop {
        let mut available = 0u32;
        unsafe {
            PeekNamedPipe(handle, None, 0, None, Some(&mut available), None)?;
        }
        if available as usize >= required {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(WindowsNativePipeError::Timeout);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn is_local_service_process(process_id: u32) -> bool {
    let Ok(process) =
        (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) })
    else {
        return false;
    };
    let process = unsafe { OwnedHandle::from_raw_handle(process.0 as *mut _) };
    let mut token = HANDLE::default();
    if unsafe { OpenProcessToken(HANDLE(process.as_raw_handle()), TOKEN_QUERY, &mut token) }
        .is_err()
    {
        return false;
    }
    let token = unsafe { OwnedHandle::from_raw_handle(token.0 as *mut _) };
    let mut needed = 0u32;
    let _ = unsafe {
        GetTokenInformation(
            HANDLE(token.as_raw_handle()),
            TokenUser,
            None,
            0,
            &mut needed,
        )
    };
    if needed == 0 {
        return false;
    }
    let mut buffer = vec![0u8; needed as usize];
    if unsafe {
        GetTokenInformation(
            HANDLE(token.as_raw_handle()),
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            needed,
            &mut needed,
        )
    }
    .is_err()
    {
        return false;
    }
    let token_user = unsafe { &*(buffer.as_ptr() as *const TOKEN_USER) };
    let sid = token_user.User.Sid;
    if sid.0.is_null() || !unsafe { IsValidSid(sid).as_bool() } {
        return false;
    }
    if unsafe { *GetSidSubAuthorityCount(sid) } != 1 {
        return false;
    }
    let authority = unsafe {
        (*sid.0.cast::<windows::Win32::Security::SID>())
            .IdentifierAuthority
            .Value
    };
    authority == [0, 0, 0, 0, 0, 5]
        && unsafe { *GetSidSubAuthority(sid, 0) } == SECURITY_LOCAL_SERVICE_RID as u32
}

impl Drop for WindowsNativePipeServer {
    fn drop(&mut self) {
        self.disconnect_client();
    }
}

pub struct WindowsNativePipeClient {
    handle: OwnedHandle,
}

impl WindowsNativePipeClient {
    pub fn is_available() -> bool {
        let name = pipe_name_wide();
        unsafe { WaitNamedPipeW(PCWSTR(name.as_ptr()), 0).as_bool() }
    }

    /// Connect to the producer only after the caller authenticates the
    /// OS-reported server process. The native protocol transfers HANDLE values,
    /// so a fixed pipe name without this reciprocal check is not a trust
    /// boundary.
    pub fn connect_with_server_validator(
        validate: impl FnOnce(u32) -> bool,
    ) -> Result<Self, WindowsNativePipeError> {
        let name = pipe_name_wide();
        let handle = unsafe {
            CreateFileW(
                PCWSTR(name.as_ptr()),
                FILE_GENERIC_READ.0 | FILE_GENERIC_WRITE.0,
                FILE_SHARE_NONE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )?
        };
        let handle = unsafe { OwnedHandle::from_raw_handle(handle.0 as *mut _) };
        let client = Self { handle };
        let mut process_id = 0u32;
        unsafe {
            GetNamedPipeServerProcessId(HANDLE(client.handle.as_raw_handle()), &mut process_id)?;
        }
        if process_id == 0 || !validate(process_id) {
            return Err(WindowsNativePipeError::PeerRejected);
        }
        Ok(client)
    }

    pub fn read_frame(&self) -> Result<Vec<u8>, WindowsNativePipeError> {
        read_frame(HANDLE(self.handle.as_raw_handle()))
    }

    pub fn read_frame_timeout(&self, timeout: Duration) -> Result<Vec<u8>, WindowsNativePipeError> {
        read_frame_timeout(HANDLE(self.handle.as_raw_handle()), timeout)
    }

    pub fn write_frame(&self, payload: &[u8]) -> Result<(), WindowsNativePipeError> {
        write_frame(HANDLE(self.handle.as_raw_handle()), payload)
    }
}
