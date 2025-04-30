import os
from pathlib import Path


def windows_long_paths_enabled():
    if os.name != "nt":
        return True
    try:
        import winreg

        with winreg.OpenKey(
            winreg.HKEY_LOCAL_MACHINE, r"SYSTEM\\CurrentControlSet\\Control\\FileSystem"
        ) as key:
            value, _ = winreg.QueryValueEx(key, "LongPathsEnabled")
            return value == 1
    except Exception:
        return False


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
if os.name != "nt" or windows_long_paths_enabled():
    file_3.write_text(file_3.name)
else:
    print("Skipping long path file creation: Windows long path support is not enabled.")
    signal_file = prefix / "files" / "long_path_test_skipped.txt"
    signal_file.touch()

file_crazy_check = prefix / "files" / "test_crazy_chars_present.txt"
file_crazy_check.touch()
