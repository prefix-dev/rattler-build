import os

import pytest

import rattler_build


@pytest.mark.skipif(not os.getenv("CI"), reason="Only run on CI")
def test_upload_to_quetz_no_token() -> None:
    url = "https://quetz.io"
    channel = "some_channel"
    with pytest.raises(rattler_build.RattlerBuildError, match="No quetz api key was given"):
        rattler_build.upload_package_to_quetz([], url, channel)


@pytest.mark.skipif(not os.getenv("CI"), reason="Only run on CI")
def test_upload_to_artifactory_no_token() -> None:
    url = "https://artifactory.io"
    channel = "some_channel"
    with pytest.raises(rattler_build.RattlerBuildError, match="No bearer token was given"):
        rattler_build.upload_package_to_artifactory([], url, channel)


@pytest.mark.skipif(not os.getenv("CI"), reason="Only run on CI")
def test_upload_to_prefix_no_token() -> None:
    url = "https://prefix.dev"
    channel = "some_channel"
    with pytest.raises(rattler_build.RattlerBuildError, match="No prefix.dev api key was given"):
        rattler_build.upload_package_to_prefix([], url, channel)


@pytest.mark.skipif(not os.getenv("CI"), reason="Only run on CI")
def test_upload_to_anaconda_no_token() -> None:
    url = "https://anaconda.org"
    with pytest.raises(rattler_build.RattlerBuildError, match="No anaconda.org api key was given"):
        rattler_build.upload_package_to_anaconda([], url)


@pytest.mark.skipif(not os.getenv("CI"), reason="Only run on CI")
def test_upload_packages_to_conda_forge_invalid_url() -> None:
    staging_token = "xxx"
    feedstock = "some_feedstock"
    feedstock_token = "xxx"
    anaconda_url = "invalid-url"

    with pytest.raises(rattler_build.RattlerBuildError, match="relative URL without a base"):
        rattler_build.upload_packages_to_conda_forge(
            [], staging_token, feedstock, feedstock_token, anaconda_url=anaconda_url
        )
