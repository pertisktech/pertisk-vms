#!/usr/bin/env python3
"""Fullscreen WebKit window for the Pertisk installer."""
import os
import sys

os.environ.setdefault("GDK_BACKEND", "wayland")

import gi

gi.require_version("Gtk", "3.0")
gi.require_version("WebKit2", "4.1")
from gi.repository import Gdk, Gtk, WebKit2


def _message_text(value) -> str:
    if value is None:
        return ""
    if isinstance(value, str):
        return value
    getter = getattr(value, "get_js_value", None)
    if callable(getter):
        value = getter()
    if hasattr(value, "is_string") and not value.is_string():
        return ""
    to_string = getattr(value, "to_string", None)
    if not callable(to_string):
        return ""
    try:
        return to_string() or ""
    except Exception:
        return ""


def _copy_text(text: str) -> None:
    text = text.strip()
    if not text:
        return
    display = Gdk.Display.get_default()
    for selection in (Gdk.SELECTION_CLIPBOARD, Gdk.SELECTION_PRIMARY):
        if display is not None:
            clip = Gtk.Clipboard.get_for_display(display, selection)
        else:
            clip = Gtk.Clipboard.get(selection)
        clip.set_text(text, -1)
        clip.store()


def _on_copy(_manager, result) -> None:
    _copy_text(_message_text(result))


def _enable_copy(view: WebKit2.WebView) -> None:
    # The installer is http://127.0.0.1. WebKit blocks navigator.clipboard
    # there, so the page asks this process to own the GTK selection.
    # A bad handler registration must not take the window down.
    try:
        settings = view.get_settings()
        if hasattr(settings, "set_javascript_can_access_clipboard"):
            settings.set_javascript_can_access_clipboard(True)
        manager = view.get_user_content_manager()
        register = getattr(manager, "register_script_message_handler", None)
        if register is None:
            return
        ok = register("pertiskCopy")
        if ok is False:
            return
        manager.connect("script-message-received::pertiskCopy", _on_copy)
    except Exception as exc:
        print(f"pertisk-kiosk: clipboard bridge skipped: {exc}", file=sys.stderr)


def main() -> int:
    url = os.environ.get("PERTISK_INSTALLER_URL", "http://127.0.0.1:8090/")
    try:
        window = Gtk.Window(title="Install Pertisk Vms")
        window.connect("destroy", Gtk.main_quit)
        window.fullscreen()
        view = WebKit2.WebView()
        _enable_copy(view)
        view.load_uri(url)
        window.add(view)
        window.show_all()
        Gtk.main()
    except Exception as exc:
        print(f"pertisk-kiosk: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
