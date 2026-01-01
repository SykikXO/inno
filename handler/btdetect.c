
#include <dbus/dbus.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

bool is_bluetooth_device_path(const char *path) {
  return path && strncmp(path, "/org/bluez/hci", 13) == 0 &&
         strstr(path, "/dev_") != NULL;
}

bool match_interfaces_added(const char *iface, const char *member,
                            const char *path) {
  return iface && strcmp(iface, "org.freedesktop.DBus.ObjectManager") == 0 &&
         member && strcmp(member, "InterfacesAdded") == 0 &&
         is_bluetooth_device_path(path);
}

void handle_interfaces_added(DBusMessage *msg) {
  DBusMessageIter args, dict_iter, entry_iter;

  if (!dbus_message_iter_init(msg, &args)) {
    printf("InterfacesAdded has no arguments.\n");
    return;
  }

  // First argument: object path
  const char *object_path = NULL;
  dbus_message_iter_get_basic(&args, &object_path);

  if (!is_bluetooth_device_path(object_path)) {
    // Not a Bluetooth device path; ignore
    return;
  }

  // Second argument: dictionary a{sa{sv}} (interfaces and properties)
  if (!dbus_message_iter_next(&args)) {
    printf("InterfacesAdded missing interfaces dict.\n");
    return;
  }

  dbus_message_iter_recurse(&args, &dict_iter);

  // Iterate through interfaces added
  while (dbus_message_iter_get_arg_type(&dict_iter) != DBUS_TYPE_INVALID) {
    dbus_message_iter_recurse(&dict_iter, &entry_iter);

    // Key = interface name (string)
    const char *interface_name = NULL;
    dbus_message_iter_get_basic(&entry_iter, &interface_name);

    if (strcmp(interface_name, "org.bluez") == 0) {
      // Move to properties dict a{sv}
      if (!dbus_message_iter_next(&entry_iter)) {
        dbus_message_iter_next(&dict_iter);
        continue;
      }

      DBusMessageIter props_iter;
      dbus_message_iter_recurse(&entry_iter, &props_iter);

      // Iterate properties for this interface
      while (dbus_message_iter_get_arg_type(&props_iter) != DBUS_TYPE_INVALID) {
        DBusMessageIter prop_entry;
        dbus_message_iter_recurse(&props_iter, &prop_entry);

        const char *prop_name = NULL;
        dbus_message_iter_get_basic(&prop_entry, &prop_name);

        if (!dbus_message_iter_next(&prop_entry)) {
          dbus_message_iter_next(&props_iter);
          continue;
        }

        DBusMessageIter variant_iter;
        dbus_message_iter_recurse(&prop_entry, &variant_iter);

        if (strcmp(prop_name, "Connected") == 0 &&
            dbus_message_iter_get_arg_type(&variant_iter) ==
                DBUS_TYPE_BOOLEAN) {
          dbus_bool_t connected = false;
          dbus_message_iter_get_basic(&variant_iter, &connected);
          printf("Device %s connected state: %s\n", object_path,
                 connected ? "true" : "false");
        }

        dbus_message_iter_next(&props_iter);
      }
    }

    dbus_message_iter_next(&dict_iter);
  }
}

int main() {
  DBusError err;
  dbus_error_init(&err);

  DBusConnection *conn = dbus_bus_get(DBUS_BUS_SYSTEM, &err);
  if (dbus_error_is_set(&err)) {
    fprintf(stderr, "Connection Error: %s\n", err.message);
    dbus_error_free(&err);
    return 1;
  }
  if (!conn) {
    fprintf(stderr, "Failed to connect to system bus\n");
    return 1;
  }

  dbus_bus_add_match(conn,
                     "type='signal',interface='org.freedesktop.DBus."
                     "ObjectManager',member='InterfacesAdded'",
                     &err);
  if (dbus_error_is_set(&err)) {
    fprintf(stderr, "Add match error: %s\n", err.message);
    dbus_error_free(&err);
    return 1;
  }

  dbus_connection_flush(conn);

  printf("Listening for Bluetooth InterfacesAdded signals...\n");

  while (true) {
    dbus_connection_read_write(conn, -1);
    DBusMessage *msg = dbus_connection_pop_message(conn);
    if (!msg)
      continue;

    if (match_interfaces_added(dbus_message_get_interface(msg),
                               dbus_message_get_member(msg),
                               dbus_message_get_path(msg))) {
      handle_interfaces_added(msg);
    }

    dbus_message_unref(msg);
  }

  return 0;
}
