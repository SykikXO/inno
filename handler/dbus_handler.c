#include "dbus_handler.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <stdint.h>

typedef struct {
    SystemEventHandler callback;
    void *user_data;
} HandlerContext;

static HandlerContext g_ctx;

int dbus_handler_init(SystemEventHandler callback, void *user_data) {
    g_ctx.callback = callback;
    g_ctx.user_data = user_data;
    
    DBusError err;
    dbus_error_init(&err);

    DBusConnection *conn = dbus_bus_get(DBUS_BUS_SYSTEM, &err);
    if (dbus_error_is_set(&err)) {
        fprintf(stderr, "Connection error: %s\n", err.message);
        dbus_error_free(&err);
        return -1;
    }
    if (!conn) {
        fprintf(stderr, "Failed to get system bus\n");
        return -1;
    }

    // Subscribe to UPower PropertiesChanged
    dbus_bus_add_match(conn,
                     "type='signal', "
                     "interface='org.freedesktop.DBus.Properties',member='PropertiesChanged'",
                     &err);
    
    // Subscribe to ObjectManager for InterfacesAdded (useful for Bluetooth/USB)
    dbus_bus_add_match(conn,
                     "type='signal',interface='org.freedesktop.DBus.ObjectManager',member='InterfacesAdded'",
                     &err);
     
     // InterfacesRemoved
    dbus_bus_add_match(conn,
                     "type='signal',interface='org.freedesktop.DBus.ObjectManager',member='InterfacesRemoved'",
                     &err);


    if (dbus_error_is_set(&err)) {
        fprintf(stderr, "Match error: %s\n", err.message);
        dbus_error_free(&err);
        return -1;
    }
    dbus_connection_flush(conn);
    return 0;
}

DBusConnection* dbus_handler_get_connection() {
    // In a real app we might want to store this in a struct passed to init, 
    // but for now re-getting the system bus connection is usually safe as it's shared.
    // However, better to just return the one we created.
    // Let's modify init to return it? 
    // For now, let's just re-get it since dbus_bus_get returns the shared connection.
    DBusError err;
    dbus_error_init(&err);
    return dbus_bus_get(DBUS_BUS_SYSTEM, &err);
}

int dbus_handler_process(DBusConnection *conn) {
    dbus_connection_read_write(conn, 0);
    DBusMessage *msg = dbus_connection_pop_message(conn);

    if (msg == NULL) return 0;

    const char *interface = dbus_message_get_interface(msg);
    const char *member = dbus_message_get_member(msg);

    // Generic logging to file
    FILE *log_file = fopen("inno_debug.log", "a");
    if (log_file) {
        const char *info_path = dbus_message_get_path(msg) ? dbus_message_get_path(msg) : "unknown_path";
        fprintf(log_file, "Signal: %s : %s | Path: %s\n", interface ? interface : "nil", member ? member : "nil", info_path);
        fclose(log_file);
    }

    // Filter UPower events
    if (interface && strcmp(interface, "org.freedesktop.DBus.Properties") == 0 &&
        member && strcmp(member, "PropertiesChanged") == 0) {
        
        DBusMessageIter iter;
        if (dbus_message_iter_init(msg, &iter)) {
             // 1. Interface name
            if (dbus_message_iter_get_arg_type(&iter) == DBUS_TYPE_STRING) {
                const char *iface_name;
                dbus_message_iter_get_basic(&iter, &iface_name);
                
                // We only care about UPower device properties
                if (strstr(iface_name, "org.freedesktop.UPower.Device")) {
                     // Check dictionary for "State" or "Percentage"
                     if (!dbus_message_iter_next(&iter)) {
                         dbus_message_unref(msg);
                         return 0;
                     }
                     
                     if (dbus_message_iter_get_arg_type(&iter) != DBUS_TYPE_ARRAY) {
                        dbus_message_unref(msg);
                        return 0;
                     }
                     
                     DBusMessageIter dict_iter;
                     dbus_message_iter_recurse(&iter, &dict_iter);
                     
                     char notification_buffer[128];
                     int has_update = 0;
                     
                     while (dbus_message_iter_get_arg_type(&dict_iter) == DBUS_TYPE_DICT_ENTRY) {
                         DBusMessageIter entry_iter;
                         dbus_message_iter_recurse(&dict_iter, &entry_iter);
                         
                         const char *key;
                         dbus_message_iter_get_basic(&entry_iter, &key);
                         dbus_message_iter_next(&entry_iter);
                         
                         DBusMessageIter variant_iter;
                         dbus_message_iter_recurse(&entry_iter, &variant_iter);
                         
                         if (strcmp(key, "Percentage") == 0) {
                             double pct = -1.0;
                             int type = dbus_message_iter_get_arg_type(&variant_iter);
                             
                             if (type == DBUS_TYPE_DOUBLE) {
                                dbus_message_iter_get_basic(&variant_iter, &pct);
                             } else if (type == DBUS_TYPE_UINT32) {
                                uint32_t val;
                                dbus_message_iter_get_basic(&variant_iter, &val);
                                pct = (double)val;
                             } else if (type == DBUS_TYPE_INT32) {
                                int32_t val;
                                dbus_message_iter_get_basic(&variant_iter, &val);
                                pct = (double)val;
                             } else if (type == DBUS_TYPE_BYTE) {
                                unsigned char val;
                                dbus_message_iter_get_basic(&variant_iter, &val);
                                pct = (double)val;
                             }

                             if (pct >= 0) {
                                snprintf(notification_buffer, sizeof(notification_buffer), "Battery: %.0f%%", pct);
                                has_update = 1;

                                // Log
                                FILE *log = fopen("inno_debug.log", "a");
                                if (log) { fprintf(log, "Debug: Parsed Percentage: %.2f (Type: %c)\n", pct, type); fclose(log); }
                             }
                         } else if (strcmp(key, "State") == 0) {
                             uint32_t state = 0;
                             if (dbus_message_iter_get_arg_type(&variant_iter) == DBUS_TYPE_UINT32) {
                                dbus_message_iter_get_basic(&variant_iter, &state);
                                
                                // Log
                                FILE *log = fopen("inno_debug.log", "a");
                                if (log) { fprintf(log, "Debug: Parsed State: %u\n", state); fclose(log); }

                                // 1=Charging, 2=Discharging, 4=Full
                                if (state == 1) {
                                    snprintf(notification_buffer, sizeof(notification_buffer), "Charging");
                                    has_update = 1;
                                } else if (state == 2) {
                                    snprintf(notification_buffer, sizeof(notification_buffer), "Discharging");
                                    has_update = 1;
                                } else if (state == 4) {
                                    snprintf(notification_buffer, sizeof(notification_buffer), "Battery Full");
                                    has_update = 1;
                                }
                             }
                         }
                         
                         dbus_message_iter_next(&dict_iter);
                     }
                     
                     if (has_update && g_ctx.callback) {
                         g_ctx.callback(notification_buffer, g_ctx.user_data);
                     }
                }
            }
        }
    }
    
    // Get Path for Interfaces events
    const char *obj_path = dbus_message_get_path(msg);

    // Handle InterfacesAdded (Device Connected)
    if (member && strcmp(member, "InterfacesAdded") == 0) {
        if (obj_path) {
             // Example filtering: Only notify if it's a battery or line power
             // UPower paths often look like /org/freedesktop/UPower/devices/line_power_AC
             if (strstr(obj_path, "UPower")) {
                  char buf[256];
                  snprintf(buf, sizeof(buf), "Connected: %s", obj_path);
                  if (g_ctx.callback) g_ctx.callback(buf, g_ctx.user_data);
             }
        }
    }
    
    // Handle InterfacesRemoved (Device Disconnected)
    if (member && strcmp(member, "InterfacesRemoved") == 0) {
        if (obj_path) {
             // Filter
             if (strstr(obj_path, "UPower")) {
                  char buf[256];
                  snprintf(buf, sizeof(buf), "Disconnected: %s", obj_path);
                  if (g_ctx.callback) g_ctx.callback(buf, g_ctx.user_data);
             }
        }
    }

    dbus_message_unref(msg);
    return 1;
}

void dbus_handler_cleanup(DBusConnection *conn) {
    if (conn) dbus_connection_unref(conn);
}
