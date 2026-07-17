$architecture = [System.Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture
Write-Output "ProcessArchitecture=$($architecture.ToString().ToUpperInvariant())"

# This reports the architecture of this PowerShell process, independently from
# PROCESSOR_ARCHITECTURE. It follows conda-build's Windows ARM integration test.
if ($architecture -ne [System.Runtime.InteropServices.Architecture]::Arm64) {
    throw "Expected an ARM64 process, got $architecture."
}
