#!/usr/bin/env python3
"""Install only the new Dev supervisor."""
from install_dev import install_components

if __name__ == "__main__":
    install_components(["ptyd"])
