#include "wayland_init.h"
#include <stdio.h>
#include <string.h>

static void handle_global(void *data, struct wl_registry *registry,
                          uint32_t name, const char *interface,
                          uint32_t version) {
  struct WaylandState *state = data;

  if (strcmp(interface, wl_compositor_interface.name) == 0)
    state->compositor =
        wl_registry_bind(registry, name, &wl_compositor_interface, 4);

  else if (strcmp(interface, wl_shm_interface.name) == 0)
    state->shm = wl_registry_bind(registry, name, &wl_shm_interface, 1);

  else if (strcmp(interface, xdg_wm_base_interface.name) == 0)
    state->wm_base =
        wl_registry_bind(registry, name, &xdg_wm_base_interface, 1);
}

static void handle_global_remove(void *data, struct wl_registry *registry,
                                 uint32_t name) {}

static struct wl_registry_listener registry_listener = {
    .global = handle_global, .global_remove = handle_global_remove};

int wayland_init(struct WaylandState *state) {
  state->display = wl_display_connect(NULL);
  if (!state->display)
    return -1;

  state->registry = wl_display_get_registry(state->display);
  wl_registry_add_listener(state->registry, &registry_listener, state);
  wl_display_roundtrip(state->display);

  if (!state->compositor || !state->shm || !state->wm_base)
    return -2;
  return 0;
}

void wayland_cleanup(struct WaylandState *state) {
  if (state->display)
    wl_display_disconnect(state->display);
}
