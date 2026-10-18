#!/usr/bin/env python3
"""Fullscreen WebKit window for the Pertisk installer."""
import os
import sys

os.environ.setdefault("GDK_BACKEND", "wayland")

import gi

gi.require_version("Gtk", "3.0")
gi.require_version("WebKit2", "4.1")
from gi.repository import Gtk, WebKit2


def main() -> int:
    url = os.environ.get("PERTISK_INSTALLER_URL", "http://127.0.0.1:8090/")
    window = Gtk.Window(title="Install Pertisk Vms")
    window.connect("destroy", Gtk.main_quit)
    window.fullscreen()
    view = WebKit2.WebView()
    view.load_uri(url)
    window.add(view)
    window.show_all()
    Gtk.main()
    return 0


if __name__ == "__main__":
    sys.exit(main())
