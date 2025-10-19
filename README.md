# inno

a extremely lightweight notification agent, writted in c
made to provide low battery, charging, full charge, device connected(ble/usb),
device disconnected notifs.

usage : 
wayland-scanner client-header /usr/share/wayland-protocols/stable/xdg-shell/xdg-shell.xml xdg-shell-client-protocol.h
wayland-scanner private-code /usr/share/wayland-protocols/stable/xdg-shell/xdg-shell.xml xdg-shell-protocol.c

use commands to generate xdg-shell-client-protocol.h and xdg-shell-protocol.c in current directory.

then compile using make in projects root directory.

references:
https://www.youtube.com/watch?v=iIVIu7YRdY0
https://www.youtube.com/watch?v=lw4P1Oup5LQ
