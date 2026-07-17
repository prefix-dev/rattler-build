Add-Type @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;

public static class NativeMethods
{
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern IntPtr GetCurrentProcess();

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool IsWow64Process2(
        IntPtr process,
        out ushort processMachine,
        out ushort nativeMachine);
}
'@

[uint16] $processMachine = 0
[uint16] $nativeMachine = 0
if (-not [NativeMethods]::IsWow64Process2(
    [NativeMethods]::GetCurrentProcess(),
    [ref] $processMachine,
    [ref] $nativeMachine
)) {
    throw [Win32Exception]::new([Runtime.InteropServices.Marshal]::GetLastWin32Error())
}

# IMAGE_FILE_MACHINE_UNKNOWN means the process is native. IMAGE_FILE_MACHINE_ARM64
# is 0xAA64. Checking these values proves the child architecture independently
# from PROCESSOR_ARCHITECTURE.
if ($processMachine -ne 0 -or $nativeMachine -ne 0xAA64) {
    throw "Expected a native ARM64 process, got process machine 0x{0:X4} and native machine 0x{1:X4}." -f $processMachine, $nativeMachine
}
