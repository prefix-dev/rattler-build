from urllib.request import urlopen

import pytest
import rattler_build.rattler_build as _rb
import rattler_build.recipe_generation as rg


def _network_available(url: str = "https://example.com") -> bool:
    try:
        with urlopen(url, timeout=5):
            return True
    except Exception:
        return False


@pytest.mark.skipif(not _network_available("https://pypi.org/pypi/pip/json"), reason="Network not available for PyPI")
def test_generate_pypi_recipe_string_smoke() -> None:
    s = _rb.generate_pypi_recipe_string_py("flit-core")
    assert "package:" in s
    assert "about:" in s
    assert "source:" in s


@pytest.mark.skipif(
    not _network_available("https://r-universe.dev"), reason="Network not available for CRAN/R-universe"
)
def test_generate_cran_recipe_string_smoke() -> None:
    s = _rb.generate_r_recipe_string_py("assertthat")
    assert "package:" in s
    assert "about:" in s
    assert "source:" in s


@pytest.mark.skipif(not _network_available("https://fastapi.metacpan.org"), reason="Network not available for MetaCPAN")
def test_generate_cpan_recipe_string_smoke() -> None:
    s = _rb.generate_cpan_recipe_string_py("Try-Tiny")
    assert "package:" in s
    assert "about:" in s
    assert "source:" in s


@pytest.mark.skipif(not _network_available("https://luarocks.org"), reason="Network not available for LuaRocks")
def test_generate_luarocks_recipe_string_smoke() -> None:
    s = _rb.generate_luarocks_recipe_string_py("luafilesystem")
    assert "package:" in s
    assert "about:" in s
    assert "source:" in s


@pytest.mark.skipif(not _network_available("https://pypi.org/pypi/pip/json"), reason="Network not available for PyPI")
def test_pypi_wrapper_smoke() -> None:
    s = rg.pypi("flit-core")
    assert "package:" in s
    assert "about:" in s
    assert "source:" in s


@pytest.mark.skipif(not _network_available("https://pypi.org/pypi/pip/json"), reason="Network not available for PyPI")
def test_pypi_wrapper_with_version() -> None:
    s = rg.pypi("flit-core", version=None)
    assert "package:" in s
    assert "about:" in s
    assert "source:" in s


@pytest.mark.skipif(
    not _network_available("https://r-universe.dev"), reason="Network not available for CRAN/R-universe"
)
def test_cran_wrapper_smoke() -> None:
    s = rg.cran("assertthat")
    assert "package:" in s
    assert "about:" in s
    assert "source:" in s


@pytest.mark.skipif(
    not _network_available("https://r-universe.dev"), reason="Network not available for CRAN/R-universe"
)
def test_cran_wrapper_with_universe() -> None:
    s = rg.cran("assertthat", universe=None)
    assert "package:" in s
    assert "about:" in s
    assert "source:" in s


@pytest.mark.skipif(not _network_available("https://fastapi.metacpan.org"), reason="Network not available for MetaCPAN")
def test_cpan_wrapper_smoke() -> None:
    s = rg.cpan("Try-Tiny")
    assert "package:" in s
    assert "about:" in s
    assert "source:" in s


@pytest.mark.skipif(not _network_available("https://fastapi.metacpan.org"), reason="Network not available for MetaCPAN")
def test_cpan_wrapper_with_version() -> None:
    s = rg.cpan("Try-Tiny", version=None)
    assert "package:" in s
    assert "about:" in s
    assert "source:" in s


@pytest.mark.skipif(not _network_available("https://luarocks.org"), reason="Network not available for LuaRocks")
def test_luarocks_wrapper_smoke() -> None:
    s = rg.luarocks("luafilesystem")
    assert "package:" in s
    assert "about:" in s
    assert "source:" in s
