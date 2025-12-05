from pathlib import Path

from rattler_build._rattler_build import (
    upload_package_to_anaconda_py,
    upload_package_to_artifactory_py,
    upload_package_to_prefix_py,
    upload_package_to_quetz_py,
    upload_packages_to_conda_forge_py,
)

__all__ = [
    "upload_package_to_quetz",
    "upload_package_to_artifactory",
    "upload_package_to_prefix",
    "upload_package_to_anaconda",
    "upload_packages_to_conda_forge",
]


def upload_package_to_quetz(
    package_files: list[str],
    url: str,
    channels: str,
    api_key: str | None = None,
    auth_file: str | Path | None = None,
) -> None:
    """
    Upload to a Quetz server. Authentication is used from the keychain / auth-file.

    Args:
        package_files: The package files to upload.
        url: The URL of the Quetz server.
        channels: The channels to upload the package to.
        api_key: The API key for authentication.
        auth_file: The authentication file.

    Returns:
        None
    """
    upload_package_to_quetz_py(package_files, url, channels, api_key, auth_file)


def upload_package_to_artifactory(
    package_files: list[str],
    url: str,
    channels: str,
    token: str | None = None,
    auth_file: str | Path | None = None,
) -> None:
    """
    Upload to an Artifactory channel. Authentication is used from the keychain / auth-file.

    Args:
        package_files: The package files to upload.
        url: The URL to your Artifactory server.
        channels: The URL to your channel.
        token: Your Artifactory token.
        auth_file: The authentication file.

    Returns:
        None
    """
    upload_package_to_artifactory_py(package_files, url, channels, token, auth_file)


def upload_package_to_prefix(
    package_files: list[str],
    url: str,
    channels: str,
    api_key: str | None = None,
    auth_file: str | Path | None = None,
    skip_existing: bool = False,
    generate_attestation: bool = False,
    attestation_file: str | Path | None = None,
) -> None:
    """
    Upload to a prefix.dev server. Authentication is used from the keychain / auth-file.

    Args:
        package_files: The package files to upload.
        url: The URL to the prefix.dev server (only necessary for self-hosted instances).
        channels: The channel to upload the package to.
        api_key: The prefix.dev API key, if none is provided, the token is read from the keychain / auth-file.
        auth_file: The authentication file.
        skip_existing: Skip upload if package is existed.
        generate_attestation: Whether to generate an attestation for the uploaded packages.
        attestation_file: Path to an attestation file to upload along with the packages (note: only a single package can be uploaded when using this).

    Returns:
        None
    """
    upload_package_to_prefix_py(
        package_files, url, channels, api_key, auth_file, skip_existing, generate_attestation, attestation_file
    )


def upload_package_to_anaconda(
    package_files: list[str],
    owner: str,
    channel: list[str] | None = None,
    api_key: str | None = None,
    url: str | None = None,
    force: bool = False,
    auth_file: str | Path | None = None,
) -> None:
    """
    Upload to an Anaconda.org server.

    Args:
        package_files: The package files to upload.
        owner: The owner of the Anaconda.org account.
        channel: The channels to upload the package to.
        api_key: The Anaconda.org API key.
        url: The URL to the Anaconda.org server.
        force: Whether to force the upload.
        auth_file: The authentication file.

    Returns:
        None
    """
    upload_package_to_anaconda_py(package_files, owner, channel, api_key, url, force, auth_file)


def upload_packages_to_conda_forge(
    package_files: list[str | Path],
    staging_token: str,
    feedstock: str,
    feedstock_token: str,
    staging_channel: str | None = None,
    anaconda_url: str | None = None,
    validation_endpoint: str | None = None,
    provider: str | None = None,
    dry_run: bool = False,
) -> None:
    """
    Upload to conda forge.

    Args:
        package_files: The package files to upload.
        staging_token: The staging token for conda forge.
        feedstock: The feedstock repository.
        feedstock_token: The feedstock token.
        staging_channel: The staging channel for the upload.
        anaconda_url: The URL to the Anaconda.org server.
        validation_endpoint: The validation endpoint.
        provider: The provider for the upload.
        dry_run: Whether to perform a dry run.

    Returns:
        None
    """
    upload_packages_to_conda_forge_py(
        package_files,
        staging_token,
        feedstock,
        feedstock_token,
        staging_channel,
        anaconda_url,
        validation_endpoint,
        provider,
        dry_run,
    )
