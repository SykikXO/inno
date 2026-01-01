#ifndef DBUS_HANDLER_H
#define DBUS_HANDLER_H

#include <dbus/dbus.h>

/* Callback for when a relevant system event occurs */
typedef void (*SystemEventHandler)(const char *event_name, void *user_data);

/* Initialize DBus connection */
int dbus_handler_init(SystemEventHandler callback, void *user_data);

/* Get DBus watch file descriptor (simplified for integration) 
   Note: libdbus usually needs a set of watches. For simplicity we assume simpler usage or
   we just return the connection dispatch status manually in the loop. 
   Actually, the best way for single FD is to use dbus_connection_get_dispatch_status or similar
   but we can't easily get a single epoll-able FD from libdbus without some glue. 
   We will return the connection object so main loop can manage it.
*/
DBusConnection* dbus_handler_get_connection();

/* Process incoming DBus messages. Returns 1 if valid event dispatched, 0 otherwise. */
int dbus_handler_process(DBusConnection *conn);

/* Cleanup */
void dbus_handler_cleanup(DBusConnection *conn);

#endif
