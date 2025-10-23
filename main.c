#include "cairo_text.h"
#include "render.h"
#include "wayland_init.h"
#include "wlr-layer-shell-unstable-v1-client-protocol.h"
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/poll.h>
#include <sys/stat.h>
#include <unistd.h>
#include <wayland-client-protocol.h>

// Globals
struct zwlr_layer_shell_v1 *layer_shell = NULL;
struct WaylandState wl = {0};
struct wl_surface *surface;
struct wl_buffer *buffer;
struct RenderBuffer render_buffer;
struct wl_shm_pool *shm_pool;
uint32_t width = 400;
uint32_t height = 300;
static struct zwlr_layer_surface_v1 *layer_surface = NULL;
static struct CairoText cairo_text;
char *text_string = "Hello, World!";

// Registry listener binds required globals to wl struct and layer_shell
static void registry_handle_global(void *data, struct wl_registry *registry,
                                   uint32_t id, const char *interface,
                                   uint32_t version) {
  struct WaylandState *state = data;
  if (strcmp(interface, zwlr_layer_shell_v1_interface.name) == 0) {
    layer_shell =
        wl_registry_bind(registry, id, &zwlr_layer_shell_v1_interface, 1);
  } else if (strcmp(interface, wl_compositor_interface.name) == 0) {
    state->compositor =
        wl_registry_bind(registry, id, &wl_compositor_interface, version);
  } else if (strcmp(interface, wl_shm_interface.name) == 0) {
    state->shm = wl_registry_bind(registry, id, &wl_shm_interface, version);
  }
}

static struct wl_registry_listener registry_listener = {
    .global = registry_handle_global,
    .global_remove = NULL,
};

// Frame callback forward declaration
void frame_done(void *data, struct wl_callback *callback, uint32_t time);

struct wl_callback_listener callback_listener = {
    .done = frame_done,
};

// Layer surface configure handler: acknowledge and attach buffer
static void layer_surface_handle_configure(
    void *data, struct zwlr_layer_surface_v1 *layer_surface_param,
    uint32_t serial, uint32_t new_width, uint32_t new_height) {

  zwlr_layer_surface_v1_ack_configure(layer_surface_param, serial);

  if (new_width == 0 || new_height == 0)
    return;

  // Update width and height globals
  width = new_width;
  height = new_height;

  // Cleanup old buffer if exists
  if (render_buffer.buffer)
    destroy_buffer(&render_buffer);

  create_buffer(wl.shm, &render_buffer, width, height);
  int h = height;
  int w = width;
  if (render_cairo_text(text_string, &cairo_text, &w, &h) == 0) {
    memcpy(render_buffer.pixels, cairo_text.data, w * h * 4);
  } else {
    fprintf(stderr, "error in cairo text rendering.. exiting...\n");
    exit(-1);
  }
  wl_surface_attach(surface, render_buffer.buffer, 0, 0);
  wl_surface_damage(surface, 0, 0, width, height);
  wl_surface_commit(surface);
}

static void layer_surface_handle_closed(void *data,
                                        struct zwlr_layer_surface_v1 *surface) {
  // Optional: handle layer surface closed event
}

static const struct zwlr_layer_surface_v1_listener layer_surface_listener = {
    .configure = layer_surface_handle_configure,
    .closed = layer_surface_handle_closed,
};

// Frame callback handler: setup next frame callback and commit
void frame_done(void *data, struct wl_callback *callback, uint32_t time) {
  wl_callback_destroy(callback);

  struct wl_callback *new_cb = wl_surface_frame(surface);
  wl_callback_add_listener(new_cb, &callback_listener, NULL);
  wl_surface_attach(surface, render_buffer.buffer, 0, 0);
  wl_surface_damage(surface, 0, 0, width, height);
  wl_surface_commit(surface);
}

int main() {
  wl.display = wl_display_connect(NULL);
  if (!wl.display) {
    fprintf(stderr, "Failed to connect to Wayland display\n");
    return -1;
  }

  wl.registry = wl_display_get_registry(wl.display);
  wl_registry_add_listener(wl.registry, &registry_listener, &wl);
  wl_display_roundtrip(wl.display);

  if (!layer_shell || !wl.compositor || !wl.shm) {
    fprintf(stderr, "Missing required globals\n");
    return -1;
  }

  surface = wl_compositor_create_surface(wl.compositor);
  if (!surface) {
    fprintf(stderr, "Failed to create surface\n");
    return -1;
  }

  layer_surface = zwlr_layer_shell_v1_get_layer_surface(
      layer_shell, surface, NULL, ZWLR_LAYER_SHELL_V1_LAYER_OVERLAY,
      "simple_white_window");

  zwlr_layer_surface_v1_add_listener(layer_surface, &layer_surface_listener,
                                     NULL);

  // Key fix: anchor both left and right edges for zero x offset
  zwlr_layer_surface_v1_set_anchor(layer_surface,
                                   ZWLR_LAYER_SURFACE_V1_ANCHOR_TOP |
                                       ZWLR_LAYER_SURFACE_V1_ANCHOR_BOTTOM |
                                       ZWLR_LAYER_SURFACE_V1_ANCHOR_LEFT |
                                       ZWLR_LAYER_SURFACE_V1_ANCHOR_RIGHT);

  zwlr_layer_surface_v1_set_margin(layer_surface, 0, 0, 0, 0);
  zwlr_layer_surface_v1_set_exclusive_zone(layer_surface, -1);
  zwlr_layer_surface_v1_set_size(layer_surface, 156, 24);
  wl_surface_commit(surface);

  // Setup initial frame callback listener
  struct wl_callback *callback = wl_surface_frame(surface);
  wl_callback_add_listener(callback, &callback_listener, NULL);

  int wl_fd = wl_display_get_fd(wl.display);
  struct pollfd fds = {.fd = wl_fd, .events = POLLIN};

  while (1) {
    int ret = poll(&fds, 1, 1000);
    if (ret < 0) {
      perror("poll");
      break;
    }
    if (ret > 0 && (fds.revents & POLLIN)) {
      if (wl_display_dispatch(wl.display) == -1) {
        fprintf(stderr, "Wayland server disconnected\n");
        break;
      }
    }
    while (wl_display_dispatch_pending(wl.display) > 0) {
    }
    wl_display_flush(wl.display);
  }
  destroy_cairo_text(&cairo_text);
  destroy_buffer(&render_buffer);
  wayland_cleanup(&wl);
  return 0;
}
