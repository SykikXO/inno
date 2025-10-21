#include "cairo_image.h"
#include "cairo_text.h"
#include "image.h"
#include "render.h"
#include "stb/stb_image.h"
#include "string.h"
#include "wayland_init.h"
#include "xdg-shell-client-protocol.h"
#include <stdio.h>
#include <stdlib.h>
#include <sys/poll.h>
#include <unistd.h>
#include <wayland-client-protocol.h>

// Global variables (or wrap in a struct as needed)
struct WaylandState wl;
struct wl_surface *surface;
struct RenderBuffer buffer;

// Ping event handler: respond immediately to avoid unresponsive popup
void sh_ping(void *data, struct xdg_wm_base *wm_base, uint32_t serial) {
  xdg_wm_base_pong(wm_base, serial);
}

// Frame callback to redraw and request a new callback
void frame_new(void *data, struct wl_callback *callback, uint32_t time);

struct wl_callback_listener cb_listener = {
    .done = frame_new,
};

void frame_new(void *data, struct wl_callback *callback, uint32_t time) {
  wl_callback_destroy(callback);
  struct wl_callback *new_cb = wl_surface_frame(surface);
  wl_callback_add_listener(new_cb, &cb_listener, NULL);

  // Redraw your image or content here
  // For simplicity, we just re-attach the existing buffer
  wl_surface_attach(surface, buffer.buffer, 0, 0);
  wl_surface_damage_buffer(surface, 0, 0, buffer.width, buffer.height);
  wl_surface_commit(surface);
}

int main() {
  if (wayland_init(&wl) != 0) {
    fprintf(stderr, "Failed to initialize Wayland\n");
    return -1;
  }

  surface = wl_compositor_create_surface(wl.compositor);
  struct xdg_surface *xdg_surface =
      xdg_wm_base_get_xdg_surface(wl.wm_base, surface);
  struct xdg_toplevel *toplevel = xdg_surface_get_toplevel(xdg_surface);
  xdg_toplevel_set_title(toplevel, "Modular Wayland Client");

  // Setup ping listener
  struct xdg_wm_base_listener sh_list = {.ping = sh_ping};
  xdg_wm_base_add_listener(wl.wm_base, &sh_list, NULL);

  // cairo text rendering logic
  struct CairoText cairo_text;
  const char *text = "Hello, World!";
  if (render_cairo_text(text, &cairo_text) < 0) {
    fprintf(stderr, "Failed to render cairo text\n");
    return -1;
  }
  create_buffer(wl.shm, &buffer, cairo_text.width, cairo_text.height);
  wl_surface_attach(surface, buffer.buffer, 0, 0);
  memcpy(buffer.pixels, cairo_text.data,
         cairo_text.width * cairo_text.height * 4);

  // if (load_cairo_image("/home/sykik/.config/walls/catpuccin_samurai.png",
  //                     &cairo_image) < 0) {
  //  fprintf(stderr, "Failed to load Cairo image\n");
  //  return -1;
  //}
  // Use cairo_image.data as the buffer for wl_surface_attach or copy into shm
  // buffer
  // create_buffer(wl.shm, &buffer, cairo_image.width, cairo_image.height);
  // wl_surface_attach(surface, buffer.buffer, 0, 0);
  // Assuming buffer.pixel points to shared memory,
  // copy cairo_image.data pixel-by-pixel into buffer.pixel here before
  // attaching
  // memcpy(buffer.pixels, cairo_image.data,
  //       cairo_image.width * cairo_image.height * 4); // RGBA 4 bytes per px

  // load image logic
  // load_image("/home/sykik/.config/walls/catpuccin_samurai.png",
  // buffer.pixels,
  //           800, 600);

  // Initial surface commit with buffer attached
  wl_surface_attach(surface, buffer.buffer, 0, 0);
  wl_surface_damage_buffer(surface, 0, 0, buffer.width, buffer.height);
  wl_surface_commit(surface);

  // Setup initial frame callback
  struct wl_callback *cb = wl_surface_frame(surface);
  wl_callback_add_listener(cb, &cb_listener, NULL);

  // Improved event loop: non-blocking dispatch + flush
  int wl_fd = wl_display_get_fd(wl.display);
  struct pollfd fds = {
      .fd = wl_fd,
      .events = POLLIN,
  };

  while (1) {
    int ret = poll(&fds, 1, 1000);
    if (ret < 0) {
      perror("poll");
      break;
    } else if (ret > 0) {
      if (fds.revents & POLLIN) {
        // Blocking call: read and dispatch all available Wayland
        // events
        if (wl_display_dispatch(wl.display) == -1) {
          fprintf(stderr, "Wayland connection closed\n");
          break;
        }
      }
    } else {
      // timeout: dispatch any pending events without blocking
      while (wl_display_dispatch_pending(wl.display) > 0) {
      }
    }
    wl_display_flush(wl.display);
  }
  destroy_cairo_text(&cairo_text);
  // destroy_cairo_image(&cairo_image);
  destroy_buffer(&buffer);
  wayland_cleanup(&wl);
  return 0;
}
