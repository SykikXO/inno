#ifndef LAYER_HANDLER_H
#define LAYER_HANDLER_H

#include <stdint.h>
#include "../src/config.h"

/* Initialize the Wayland layer shell surface */
int layer_init(AppConfig *cfg);

/* Cleanup resources */
void layer_cleanup();

/* Show a notification with the given text */
void layer_show_text(const char *text);

/* Hide the notification */
void layer_hide();

/* Process Wayland events (dispatch) */
int layer_dispatch();

/* Get the Wayland display file descriptor for polling */
int layer_get_fd();

#endif
