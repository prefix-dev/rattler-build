use rattler_conda_types::Platform;

/// Requested Windows child process architecture for a supported transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowsMachine {
    X86,
    Amd64,
    Arm64,
}

impl WindowsMachine {
    pub(crate) fn start_argument(self) -> &'static str {
        match self {
            Self::X86 => "x86",
            Self::Amd64 => "amd64",
            Self::Arm64 => "arm64",
        }
    }

    pub(crate) fn processor_architecture(self) -> &'static str {
        match self {
            Self::X86 => "x86",
            Self::Amd64 => "AMD64",
            Self::Arm64 => "ARM64",
        }
    }

    /// The `PROCESSOR_ARCHITEW6432` marker Windows exposes to an x86 child.
    pub(crate) fn wow64_processor_architecture(self) -> Option<&'static str> {
        (self != Self::X86).then(|| self.processor_architecture())
    }

    #[cfg(windows)]
    fn from_image_file_machine(machine: u16) -> Option<Self> {
        use windows_sys::Win32::System::SystemInformation::{
            IMAGE_FILE_MACHINE_AMD64, IMAGE_FILE_MACHINE_ARM64, IMAGE_FILE_MACHINE_I386,
        };

        match machine {
            IMAGE_FILE_MACHINE_I386 => Some(Self::X86),
            IMAGE_FILE_MACHINE_AMD64 => Some(Self::Amd64),
            IMAGE_FILE_MACHINE_ARM64 => Some(Self::Arm64),
            _ => None,
        }
    }
}

/// Returns the requested child architecture when a supported Windows build
/// process transition is needed. Rattler-build ships x64 and ARM64 binaries,
/// and both can launch x86 build tools; x86 rattler-build processes are not
/// supported as cross-architecture launchers.
pub(crate) fn windows_machine_transition(
    process_platform: Platform,
    build_platform: Platform,
) -> Option<WindowsMachine> {
    match (process_platform, build_platform) {
        (Platform::Win64, Platform::Win32) | (Platform::WinArm64, Platform::Win32) => {
            Some(WindowsMachine::X86)
        }
        (Platform::Win64, Platform::WinArm64) => Some(WindowsMachine::Arm64),
        (Platform::WinArm64, Platform::Win64) => Some(WindowsMachine::Amd64),
        _ => None,
    }
}

/// Detects the native Windows machine architecture without affecting launch
/// selection. This is only used to reproduce the `PROCESSOR_ARCHITEW6432`
/// value that Windows exposes to x86 WOW64 processes.
#[cfg(windows)]
pub(crate) fn native_windows_machine() -> Option<WindowsMachine> {
    use windows_sys::Win32::System::{
        SystemInformation::IMAGE_FILE_MACHINE_UNKNOWN,
        Threading::{GetCurrentProcess, IsWow64Process2},
    };

    let mut process_machine = IMAGE_FILE_MACHINE_UNKNOWN;
    let mut native_machine = IMAGE_FILE_MACHINE_UNKNOWN;
    // `IsWow64Process2` is available on every Windows version that supports
    // `start /machine`. A failure leaves the inherited WOW64 marker untouched.
    if unsafe {
        IsWow64Process2(
            GetCurrentProcess(),
            &mut process_machine,
            &mut native_machine,
        )
    } == 0
    {
        return None;
    }

    WindowsMachine::from_image_file_machine(native_machine)
}

#[cfg(not(windows))]
pub(crate) fn native_windows_machine() -> Option<WindowsMachine> {
    None
}
