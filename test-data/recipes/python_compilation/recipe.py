import os
from pathlib import Path

prefix = Path(os.environ["PREFIX"])

py_content = "print('Hello, world!')"
py_file = prefix / "test.py"

py_file.write_text(py_content)

py_skippy = prefix / "test_skippy.py"
py_skippy.write_text(py_content)

new_folder = prefix / "cmd"
new_folder.mkdir(exist_ok=True)

py_file = prefix / "cmd" / "test.py"
py_file.write_text(py_content)

py_broken = prefix / "broken.py"
py_broken.write_text("print('Hello, world!'")

adding_pyc_file = prefix / "just_a_.cpython-311.pyc"
adding_pyc_file.write_text("")
