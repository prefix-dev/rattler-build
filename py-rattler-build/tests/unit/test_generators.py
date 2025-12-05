from urllib.request import urlopen

import pytest

from rattler_build import (
    generate_cpan_recipe,
    generate_cran_recipe,
    generate_luarocks_recipe,
    generate_pypi_recipe,
)


def _network_available(url: str = "https://example.com") -> bool:
    try:
        with urlopen(url, timeout=5):
            return True
    except Exception:
        return False


@pytest.mark.skipif(not _network_available("https://pypi.org/pypi/pip/json"), reason="Network not available for PyPI")
def test_generate_pypi_recipe_smoke() -> None:
    s = generate_pypi_recipe("flit-core")
    assert "package:" in s
    assert "about:" in s
    assert "source:" in s


@pytest.mark.skipif(
    not _network_available("https://r-universe.dev"), reason="Network not available for CRAN/R-universe"
)
def test_generate_cran_recipe_smoke() -> None:
    s = generate_cran_recipe("assertthat")
    assert "package:" in s
    assert "about:" in s
    assert "source:" in s


@pytest.mark.skipif(not _network_available("https://fastapi.metacpan.org"), reason="Network not available for MetaCPAN")
def test_generate_cpan_recipe_smoke() -> None:
    s = generate_cpan_recipe("Try-Tiny")
    assert "package:" in s
    assert "about:" in s
    assert "source:" in s


@pytest.mark.skipif(not _network_available("https://luarocks.org"), reason="Network not available for LuaRocks")
def test_generate_luarocks_recipe_smoke() -> None:
    s = generate_luarocks_recipe("luafilesystem")
    assert "package:" in s
    assert "about:" in s
    assert "source:" in s


@pytest.mark.skipif(not _network_available("https://pypi.org/pypi/pip/json"), reason="Network not available for PyPI")
def test_generate_pypi_recipe_with_use_mapping_false() -> None:
    """Test PyPI recipe generation with use_mapping=False."""
    s = generate_pypi_recipe("flit-core", use_mapping=False)
    assert "package:" in s
    assert "about:" in s
    assert "source:" in s


@pytest.mark.skipif(
    not _network_available("https://r-universe.dev"), reason="Network not available for CRAN/R-universe"
)
def test_generate_cran_recipe_with_explicit_universe() -> None:
    """Test CRAN recipe generation with explicit universe parameter."""
    s = generate_cran_recipe("assertthat", universe="cran")
    assert "package:" in s
    assert "about:" in s
    assert "source:" in s
