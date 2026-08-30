//! macOS System Extension lifecycle adapter — REQ-PICOO-VCAM-006.

use std::cmp::Ordering;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use dispatch2::{DispatchQueue, DispatchQueueAttr};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol, NSString};
use objc2_system_extensions::{
    OSSystemExtensionManager, OSSystemExtensionProperties, OSSystemExtensionReplacementAction,
    OSSystemExtensionRequest, OSSystemExtensionRequestDelegate, OSSystemExtensionRequestResult,
};

pub const CAMERA_EXTENSION_BUNDLE_ID: &str = "com.haoxincode.picoo-camera.camera-extension";

const QUERY_TIMEOUT: Duration = Duration::from_secs(15);
const LIFECYCLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstalledState {
    Missing,
    Bundled,
    AwaitingApproval,
    Active,
    Uninstalling,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleOutcome {
    Completed,
    RestartRequired,
}

#[derive(Debug)]
enum RequestEvent {
    NeedsUserApproval,
    Finished(OSSystemExtensionRequestResult),
    Failed(String),
    Properties(InstalledState),
}

#[derive(Debug)]
struct RequestDelegateIvars {
    events: Sender<RequestEvent>,
}

define_class!(
    // SAFETY: NSObject imposes no additional subclassing requirements. The
    // delegate owns only a thread-safe channel sender and has no custom Drop.
    #[unsafe(super = NSObject)]
    #[thread_kind = AnyThread]
    #[ivars = RequestDelegateIvars]
    struct RequestDelegate;

    // SAFETY: NSObjectProtocol has no additional safety requirements.
    unsafe impl NSObjectProtocol for RequestDelegate {}

    // SAFETY: Every selector signature matches the generated SystemExtensions
    // protocol. The delegate never retains request-owned Objective-C objects.
    unsafe impl OSSystemExtensionRequestDelegate for RequestDelegate {
        #[unsafe(method(request:actionForReplacingExtension:withExtension:))]
        fn action_for_replacement(
            &self,
            _request: &OSSystemExtensionRequest,
            existing: &OSSystemExtensionProperties,
            extension: &OSSystemExtensionProperties,
        ) -> OSSystemExtensionReplacementAction {
            // SAFETY: SystemExtensions supplies immutable version strings for
            // both bundles during this synchronous delegate callback.
            let (existing_build, replacement_build) = unsafe {
                (
                    existing.bundleVersion().to_string(),
                    extension.bundleVersion().to_string(),
                )
            };
            if compare_bundle_versions(&replacement_build, &existing_build) == Ordering::Greater {
                OSSystemExtensionReplacementAction::Replace
            } else {
                OSSystemExtensionReplacementAction::Cancel
            }
        }

        #[unsafe(method(requestNeedsUserApproval:))]
        fn needs_user_approval(&self, _request: &OSSystemExtensionRequest) {
            let _ = self.ivars().events.send(RequestEvent::NeedsUserApproval);
        }

        #[unsafe(method(request:didFinishWithResult:))]
        fn did_finish(
            &self,
            _request: &OSSystemExtensionRequest,
            result: OSSystemExtensionRequestResult,
        ) {
            let _ = self.ivars().events.send(RequestEvent::Finished(result));
        }

        #[unsafe(method(request:didFailWithError:))]
        fn did_fail(&self, _request: &OSSystemExtensionRequest, error: &NSError) {
            let message = error.localizedDescription().to_string();
            let _ = self.ivars().events.send(RequestEvent::Failed(message));
        }

        #[unsafe(method(request:foundProperties:))]
        fn found_properties(
            &self,
            _request: &OSSystemExtensionRequest,
            properties: &NSArray<OSSystemExtensionProperties>,
        ) {
            let state = classify_properties(properties);
            let _ = self.ivars().events.send(RequestEvent::Properties(state));
        }
    }
);

// SAFETY: RequestDelegate has no thread-affine state; its sole ivar is a
// std::sync::mpsc::Sender, and callbacks are delivered on our serial queue.
unsafe impl Send for RequestDelegate {}
// SAFETY: All access to the channel sender is thread-safe and immutable.
unsafe impl Sync for RequestDelegate {}

impl RequestDelegate {
    fn new(events: Sender<RequestEvent>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(RequestDelegateIvars { events });
        // SAFETY: NSObject's `init` signature is correct and our ivars are initialized.
        unsafe { msg_send![super(this), init] }
    }
}

pub fn query_installed_state() -> Result<InstalledState, String> {
    let (events, receiver) = mpsc::channel();
    let delegate = RequestDelegate::new(events);
    let queue = DispatchQueue::new(
        "com.haoxincode.picoo-camera.system-extension.query",
        DispatchQueueAttr::SERIAL,
    );
    let identifier = NSString::from_str(CAMERA_EXTENSION_BUNDLE_ID);
    // SAFETY: The queue remains alive for the whole request and the identifier
    // is a valid extension bundle identifier owned by this application.
    let request = unsafe {
        OSSystemExtensionRequest::propertiesRequestForExtension_queue(&identifier, &queue)
    };
    submit(&request, &delegate);

    match receiver.recv_timeout(QUERY_TIMEOUT) {
        Ok(RequestEvent::Properties(state)) => Ok(state),
        Ok(RequestEvent::Failed(message)) => Err(message),
        Ok(other) => Err(format!("系统扩展状态请求返回了意外事件：{other:?}")),
        Err(mpsc::RecvTimeoutError::Timeout) => Err("检测 Camera Extension 超时".into()),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err("Camera Extension 状态请求意外结束".into())
        }
    }
}

pub fn activate() -> Result<LifecycleOutcome, String> {
    ensure_activation_location()?;
    submit_lifecycle_request(true)
}

pub fn deactivate() -> Result<LifecycleOutcome, String> {
    ensure_activation_location()?;
    submit_lifecycle_request(false)
}

fn ensure_activation_location() -> Result<(), String> {
    let executable =
        std::env::current_exe().map_err(|err| format!("无法定位 Picoo Camera.app：{err}"))?;
    if executable.starts_with("/Applications/") {
        return Ok(());
    }
    Err("macOS 只允许位于“应用程序”文件夹中的 App 激活 Camera Extension。请先将 Picoo Camera.app 移到 /Applications 后重新打开。".into())
}

fn submit_lifecycle_request(activate: bool) -> Result<LifecycleOutcome, String> {
    let (events, receiver) = mpsc::channel();
    let delegate = RequestDelegate::new(events);
    let queue = DispatchQueue::new(
        "com.haoxincode.picoo-camera.system-extension.lifecycle",
        DispatchQueueAttr::SERIAL,
    );
    let identifier = NSString::from_str(CAMERA_EXTENSION_BUNDLE_ID);
    // SAFETY: The serial queue, request, and delegate are retained until a
    // terminal callback or timeout. SystemExtensions discovers the extension
    // from the current main application bundle.
    let request = unsafe {
        if activate {
            OSSystemExtensionRequest::activationRequestForExtension_queue(&identifier, &queue)
        } else {
            OSSystemExtensionRequest::deactivationRequestForExtension_queue(&identifier, &queue)
        }
    };
    submit(&request, &delegate);
    wait_for_lifecycle(receiver)
}

fn submit(request: &OSSystemExtensionRequest, delegate: &RequestDelegate) {
    // SAFETY: RequestDelegate implements the required protocol for the entire
    // request lifetime; the caller retains it because the framework property is weak.
    unsafe {
        request.setDelegate(Some(ProtocolObject::from_ref(delegate)));
        OSSystemExtensionManager::sharedManager().submitRequest(request);
    }
}

fn wait_for_lifecycle(receiver: Receiver<RequestEvent>) -> Result<LifecycleOutcome, String> {
    loop {
        match receiver.recv_timeout(LIFECYCLE_TIMEOUT) {
            Ok(RequestEvent::NeedsUserApproval) => {
                // The framework keeps the request pending. Continue waiting for
                // approval rather than reporting a false failure to the UI.
            }
            Ok(RequestEvent::Finished(result))
                if result == OSSystemExtensionRequestResult::Completed =>
            {
                return Ok(LifecycleOutcome::Completed);
            }
            Ok(RequestEvent::Finished(result))
                if result == OSSystemExtensionRequestResult::WillCompleteAfterReboot =>
            {
                return Ok(LifecycleOutcome::RestartRequired);
            }
            Ok(RequestEvent::Finished(result)) => {
                return Err(format!("未知的系统扩展结果：{}", result.0));
            }
            Ok(RequestEvent::Failed(message)) => return Err(message),
            Ok(RequestEvent::Properties(_)) => {
                return Err("系统扩展生命周期请求返回了状态查询结果".into());
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                return Err("等待 Camera Extension 系统批准超时，请在系统设置中批准后重试".into());
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("Camera Extension 系统请求意外结束".into());
            }
        }
    }
}

fn classify_properties(properties: &NSArray<OSSystemExtensionProperties>) -> InstalledState {
    let mut state = InstalledState::Missing;
    for property in properties {
        // SAFETY: These are immutable properties supplied by SystemExtensions.
        unsafe {
            if property.isEnabled() {
                return InstalledState::Active;
            }
            if property.isAwaitingUserApproval() {
                state = InstalledState::AwaitingApproval;
            } else if property.isUninstalling() && state == InstalledState::Missing {
                state = InstalledState::Uninstalling;
            } else if state == InstalledState::Missing {
                state = InstalledState::Bundled;
            }
        }
    }
    state
}

fn compare_bundle_versions(left: &str, right: &str) -> Ordering {
    let mut left = left.split('.').map(|part| part.parse::<u64>().ok());
    let mut right = right.split('.').map(|part| part.parse::<u64>().ok());
    loop {
        match (left.next(), right.next()) {
            (None, None) => return Ordering::Equal,
            (left, right) => match left
                .flatten()
                .unwrap_or(0)
                .cmp(&right.flatten().unwrap_or(0))
            {
                Ordering::Equal => {}
                ordering => return ordering,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_timeout_is_long_enough_for_system_settings_approval() {
        assert!(LIFECYCLE_TIMEOUT >= Duration::from_secs(5 * 60));
    }

    #[test]
    fn camera_extension_identifier_matches_bundle_name() {
        assert!(CAMERA_EXTENSION_BUNDLE_ID.ends_with(".camera-extension"));
        assert!(!CAMERA_EXTENSION_BUNDLE_ID.ends_with(".systemextension"));
        assert!(!CAMERA_EXTENSION_BUNDLE_ID.contains(' '));
    }

    #[test]
    fn replacement_requires_a_strictly_newer_build_number() {
        assert_eq!(compare_bundle_versions("2", "1"), Ordering::Greater);
        assert_eq!(compare_bundle_versions("2.1", "2.0.9"), Ordering::Greater);
        assert_eq!(compare_bundle_versions("2.0", "2"), Ordering::Equal);
        assert_eq!(compare_bundle_versions("9", "10"), Ordering::Less);
    }
}
