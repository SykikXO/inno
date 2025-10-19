#pragma once
#include "xdg-shell-client-protocol.h"
#include <wayland-client.h>

struct WaylandState {
  struct wl_display *display;
  struct wl_registry *registry;
  struct wl_compositor *compositor;
  struct wl_shm *shm;
  struct xdg_wm_base *wm_base;
};

int wayland_init(struct WaylandState *state);
void wayland_cleanup(struct WaylandState *state);
