#!/bin/sh
# Apply the bundled udev rule (USB access to Pinscape Pico without root)
# right away, instead of after the next reboot or replug.
if command -v udevadm >/dev/null 2>&1; then
    udevadm control --reload-rules || true
    udevadm trigger --subsystem-match=usb || true
fi
exit 0
