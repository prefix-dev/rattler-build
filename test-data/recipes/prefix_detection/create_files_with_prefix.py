import os
import platform
from pathlib import Path

prefix = Path(os.environ["PREFIX"])

binary_data = b"\0\0binary data here "
binary_data_with_prefix = (
    binary_data + str(prefix).encode("utf-8") + b"\0\0more binary data"
)

is_binary_folder = prefix / "is_binary"
is_binary_folder.mkdir(parents=True, exist_ok=True)

(is_binary_folder / "file_with_prefix").write_bytes(binary_data_with_prefix)
(is_binary_folder / "file_without_prefix").write_bytes(binary_data)


text_data = "text data here"
text_data_with_prefix = text_data + str(prefix) + " more text data"

is_text_folder = prefix / "is_text"
is_text_folder.mkdir(parents=True, exist_ok=True)

(is_text_folder / "file_with_prefix").write_text(text_data_with_prefix)
(is_text_folder / "file_without_prefix").write_text(text_data)

force_binary_folder = prefix / "force_binary"
force_binary_folder.mkdir(parents=True, exist_ok=True)

(force_binary_folder / "file_with_prefix").write_text(text_data_with_prefix)
(force_binary_folder / "file_without_prefix").write_text(text_data)

ignore_folder = prefix / "ignore"
ignore_folder.mkdir(parents=True, exist_ok=True)

(ignore_folder / "file_with_prefix").write_bytes(binary_data_with_prefix)
(ignore_folder / "text_with_prefix").write_text(text_data_with_prefix)

force_text_folder = prefix / "force_text"
force_text_folder.mkdir(parents=True, exist_ok=True)

(force_text_folder / "file_with_prefix").write_bytes(binary_data_with_prefix)
(force_text_folder / "file_without_prefix").write_bytes(binary_data)

if platform.platform().startswith("Windows"):
    (is_text_folder / "file_with_forwardslash_prefix").write_text(
        text_data_with_prefix.replace("\\", "/")
    )
