from pathlib import Path
import pytest


@pytest.fixture
def recipes_dir() -> Path:
    current_file = Path(__file__).resolve()
    recipes_path = current_file.parents[1].joinpath("data", "recipes")
    return recipes_path
