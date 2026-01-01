#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <poll.h>
#include <signal.h>
#include <string.h>
#include <time.h>
#include "../layer/layerhandler.h"
#include "../handler/dbus_handler.h"

#include <sys/timerfd.h>

#include "../src/config.h"

// State
static int running = 1;
static int tfd = -1;
static AppConfig g_config;

void handle_signal(int sig) {
    running = 0;
}

void arm_timer() {
    struct itimerspec ts = {0};
    ts.it_value.tv_sec = 5; // 5 seconds
    timerfd_settime(tfd, 0, &ts, NULL);
}

// Callback for DBus events
void on_system_event(const char *event_name, void *user_data) {
    // Check if it looks like percentage (starts with Battery:)
    if (strncmp(event_name, "Battery:", 8) == 0) {
        // Ignored as per user request
        return;
    }

    printf("Event received: %s\n", event_name);
    // Show notification
    layer_show_text(event_name);
    arm_timer();
}

int main(int argc, char **argv) {
    signal(SIGINT, handle_signal);
    signal(SIGTERM, handle_signal);

    printf("Starting Inno Notification Agent...\n");
    
    // Load Config
    char config_path[512];
    const char *home = getenv("HOME");
    if (home) {
        snprintf(config_path, sizeof(config_path), "%s/.config/inno/inno.conf", home);
    } else {
        snprintf(config_path, sizeof(config_path), "inno.conf");
    }

    if (load_config(config_path, &g_config) == 0) {
        printf("Loaded config from %s: Font=%s, Text=0x%08X, BG=0x%08X\n", 
               config_path, g_config.font_family, g_config.text_color, g_config.bg_color);
    } else {
        printf("Failed to load config from %s, using defaults.\n", config_path);
    }

    // Init Timer
    tfd = timerfd_create(CLOCK_MONOTONIC, TFD_NONBLOCK);
    if (tfd == -1) {
        perror("timerfd_create");
        return 1;
    }

    // 1. Init Layer (Wayland)
    if (layer_init(&g_config) != 0) {
        fprintf(stderr, "Failed to initialize Wayland layer\n");
        return 1;
    }

    // 2. Init DBus
    if (dbus_handler_init(on_system_event, NULL) != 0) {
        fprintf(stderr, "Failed to initialize DBus\n");
        layer_cleanup();
        return 1;
    }

    DBusConnection *dbus_conn = dbus_handler_get_connection();
    int dbus_fd = -1;
    if (dbus_connection_get_unix_fd(dbus_conn, &dbus_fd) == FALSE) {
        fprintf(stderr, "Failed to get DBus FD\n");
    }

    int wl_fd = layer_get_fd();

    // 3. Event Loop
    struct pollfd fds[3];
    
    // Initial text
    layer_show_text("Inno Agent Active");
    arm_timer();

    while (running) {
        fds[0].fd = wl_fd;
        fds[0].events = POLLIN;
        fds[1].fd = dbus_fd;
        fds[1].events = POLLIN;
        fds[2].fd = tfd;
        fds[2].events = POLLIN;

        int ret = poll(fds, 3, 200); // 200ms timeout

        if (ret < 0) {
            if (running) perror("poll");
            break;
        }

        // Handle Wayland
        if (fds[0].revents & POLLIN) {
            if (layer_dispatch() == -1) {
                running = 0;
            }
        }

        // Handle DBus
        if (dbus_fd != -1) {
             dbus_handler_process(dbus_conn);
        }

        // Handle Timer
        if (fds[2].revents & POLLIN) {
            uint64_t exp;
            read(tfd, &exp, sizeof(uint64_t));
            layer_hide();
        }
    }

    printf("Exiting...\n");
    layer_cleanup();
    dbus_handler_cleanup(dbus_conn);
    return 0;
}
