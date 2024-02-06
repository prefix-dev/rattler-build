#!/conda-bld/placeholder_placeholder/python arguments -a -b -c
# -*- coding: utf-8 -*-
import re
import sys

from myapp.cli.core import cli

if __name__ == "__main__":
    sys.argv[0] = re.sub(r"(-script\.pyw|\.exe)?$", "", sys.argv[0])
    sys.exit(cli())
