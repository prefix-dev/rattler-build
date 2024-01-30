import os
from pathlib import Path

prefix = Path(os.environ["PREFIX"])

# create some files with crazy characters
(prefix / "files").mkdir()
file_1 = prefix / "files" / "File(Glob …).tmSnippet"
file_1.write_text(file_1.name)

file_2 = (
    prefix / "files" / "a $random_crazy file name with spaces and (parentheses).txt"
)
file_2.write_text(file_2.name)

file_3 = prefix / "files" / ("a_really_long_" + ("a" * 200) + ".txt")
file_3.write_text(file_3.name)
