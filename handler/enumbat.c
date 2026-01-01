#include <dbus/dbus.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>


void print_dict_array(DBusMessageIter *array_iter) {
    DBusMessageIter dict_iter;

    while (dbus_message_iter_get_arg_type(array_iter) == DBUS_TYPE_DICT_ENTRY) {
        dbus_message_iter_recurse(array_iter, &dict_iter);

        // First element in dict entry: key (string)
        if (dbus_message_iter_get_arg_type(&dict_iter) == DBUS_TYPE_STRING) {
            const char *key;
            dbus_message_iter_get_basic(&dict_iter, &key);
            printf("Key: %s -> ", key);
        } else {
            printf("Unexpected key type\n");
            dbus_message_iter_next(array_iter);
            continue;
        }

        dbus_message_iter_next(&dict_iter);

        // Second element in dict entry: value (variant)
        if (dbus_message_iter_get_arg_type(&dict_iter) == DBUS_TYPE_VARIANT) {
            DBusMessageIter variant_iter;
            dbus_message_iter_recurse(&dict_iter, &variant_iter);
            int vtype = dbus_message_iter_get_arg_type(&variant_iter);

            switch (vtype) {
                case DBUS_TYPE_STRING: {
                    const char *val;
                    dbus_message_iter_get_basic(&variant_iter, &val);
                    printf("%s (string)\n", val);
                    break;
                }
                case DBUS_TYPE_BOOLEAN: {
                    dbus_bool_t val;
                    dbus_message_iter_get_basic(&variant_iter, &val);
                    printf("%s (boolean)\n", val ? "true" : "false");
                    break;
                }
                case DBUS_TYPE_UINT32: {
                    uint32_t val;
                    dbus_message_iter_get_basic(&variant_iter, &val);
                    printf("%u (uint32)\n", val);
                    break;
                }
                case DBUS_TYPE_INT32: {
                    int32_t val;
                    dbus_message_iter_get_basic(&variant_iter, &val);
                    printf("%d (int32)\n", val);
                    break;
                }
                case DBUS_TYPE_DOUBLE: {
                    double val;
                    dbus_message_iter_get_basic(&variant_iter, &val);
                    printf("%f (double)\n", val);
                    break;
                }
                // Add other DBus types as needed
                default:
                    printf("Unhandled variant type: %c\n", vtype);
                    break;
            }
        } else {
            printf("Expected variant type for value\n");
        }

        dbus_message_iter_next(array_iter);
    }
}

void print_message(DBusMessage *msg) {
  const char *path = dbus_message_get_path(msg);
  const char *iface = dbus_message_get_interface(msg);
  const char *member = dbus_message_get_member(msg);
  const char *sender = dbus_message_get_sender(msg);

  printf("\nGot message:\n");
  printf("  Path: %s\n", path ? path : "(null)");
  printf("  Interface: %s\n", iface ? iface : "(null)");
  printf("  Member: %s\n", member ? member : "(null)");
  printf("  Sender: %s\n", sender ? sender : "(null)");
}

void batteryMatches(DBusError *err, DBusConnection *conn) {
  // Call EnumerateDevices on UPower to get device list
  DBusMessage *msg = dbus_message_new_method_call(
      "org.freedesktop.UPower", "/org/freedesktop/UPower",
      "org.freedesktop.UPower", "EnumerateDevices");
  if (msg == NULL) {
    fprintf(stderr, "Failed to create method call\n");
    return;
  }

  DBusMessage *reply =
      dbus_connection_send_with_reply_and_block(conn, msg, -1, err);
  dbus_message_unref(msg);

  if (dbus_error_is_set(err)) {
    fprintf(stderr, "EnumerateDevices call error: %s\n", err->message);
    dbus_error_free(err);
    return;
  }
  if (!reply) {
    fprintf(stderr, "No reply received for EnumerateDevices\n");
    return;
  }

  DBusMessageIter iter;
  if (!dbus_message_iter_init(reply, &iter) ||
      dbus_message_iter_get_arg_type(&iter) != DBUS_TYPE_ARRAY) {
    fprintf(stderr, "Unexpected reply argument type\n");
    dbus_message_unref(reply);
    return;
  }

  DBusMessageIter array_iter;
  dbus_message_iter_recurse(&iter, &array_iter);

  #define MAX_BATTERIES 10
  const char *battery_paths[MAX_BATTERIES];
  int battery_count = 0;

  while (dbus_message_iter_get_arg_type(&array_iter) == DBUS_TYPE_OBJECT_PATH) {
    const char *device_path;
    dbus_message_iter_get_basic(&array_iter, &device_path);
    printf("Found device: %s\n", device_path);

    if (strstr(device_path, "battery") != NULL) {
      if (battery_count < MAX_BATTERIES) {
        battery_paths[battery_count++] = strdup(device_path);
        printf("Battery device added: %s\n", device_path);
      } else {
        fprintf(stderr, "Too many battery devices, ignoring extra\n");
      }
    }
    dbus_message_iter_next(&array_iter);
  }
  dbus_message_unref(reply);

  if (battery_count == 0) {
    fprintf(stderr, "No battery devices found, exiting\n");
    return;
  }

  // Add D-Bus match for PropertiesChanged signals for each battery device path
  for (int i = 0; i < battery_count; i++) {
    char match_rule[512];
    // Match signals on the device path with PropertiesChanged member and
    // interface
    snprintf(match_rule, sizeof(match_rule),
             "type='signal',interface='org.freedesktop.DBus.Properties',"
             "member='PropertiesChanged',path='%s'",
             battery_paths[i]);

    dbus_bus_add_match(conn, match_rule, err);
    if (dbus_error_is_set(err)) {
      fprintf(stderr, "Match error for %s: %s\n", battery_paths[i],
              err->message);
      dbus_error_free(err);
    } else {
      printf("Match added for %s\n", battery_paths[i]);
    }
  }
  dbus_connection_flush(conn);
}

void bluetoothMatches(DBusError *err, DBusConnection *conn){
    dbus_bus_add_match(
         conn,
         "type='signal',interface='org.freedesktop.DBus.ObjectManager'"
         ",member='InterfacesRemoved'",
         err);

     dbus_bus_add_match(
         conn,
         "type='signal',interface='org.freedesktop.DBus.ObjectManager'"
         ",member='InterfacesAdded'",
         err);
}

int main() {
  //init dbus
  DBusError err;
  dbus_error_init(&err);
  DBusConnection *conn = dbus_bus_get(DBUS_BUS_SYSTEM, &err);
  if (dbus_error_is_set(&err)) {
    fprintf(stderr, "Connection error: %s\n", err.message);
    dbus_error_free(&err);
    return 1;
  }
  if (!conn) {
    fprintf(stderr, "Failed to get system bus\n");
    return 1;
  }
  //add matches
  batteryMatches(&err, conn);
  bluetoothMatches(&err, conn);

  //main logic loop
  while (1) {
    dbus_connection_read_write(conn, -1);
    DBusMessage *msg = dbus_connection_pop_message(conn);
    if (msg == NULL) {
      continue;
    }

    if (dbus_message_get_path(msg)) {
        printf("path -> %s\n",dbus_message_get_path(msg));
    }
    if(dbus_message_get_interface(msg)){
        printf("interface -> %s\n", dbus_message_get_interface(msg));
    }
    printf("\n");

    dbus_message_unref(msg);
  }

  return 0;
}
