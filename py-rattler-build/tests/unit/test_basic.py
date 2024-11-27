import rattler_build


def test_basic():
    assert rattler_build.get_rattler_build_version_py() == "0.31.0"