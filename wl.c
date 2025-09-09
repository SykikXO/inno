#include "xdg-shell-client-protocol.h"
#include "xdg-shell-protocol.c"
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>
#include <wayland-client-core.h>
#include <wayland-client-protocol.h>
#include <wayland-client.h>

struct xdg_wm_base *sh;
struct xdg_toplevel *top;
struct wl_surface *surf;
struct wl_compositor *comp;
struct wl_buffer *bfr;
struct wl_shm *shm;

uint8_t *pixl;
uint16_t w = 200;
uint16_t h = 100;

int32_t allocate_shm(uint64_t size) {
  int8_t name[8];
  name[0] = '/';
  name[7] = 0;
  for (uint8_t i = 1; i < 6; i++) {
    name[i] = (rand() & 23) + 97;
  }
  int32_t fd = shm_open(name, O_RDWR | O_CREAT | O_EXCL,
                        S_IWUSR | S_IRUSR | S_IWOTH | S_IROTH);
  shm_unlink(name);
  ftruncate(fd, size);
  return fd;
}
void resz() {
  int32_t fd = allocate_shm(w * h * 4);
  pixl = mmap(0, w * h * 4, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
  struct wl_shm_pool *pool = wl_shm_create_pool(shm, fd, w * h * 4);
  bfr = wl_shm_pool_create_buffer(pool, 0, w, h, w * 4, WL_SHM_FORMAT_ARGB8888);
  wl_shm_pool_destroy(pool);
  close(fd);
}
void draw() {}
void xsurf_conf(void *data, struct xdg_surface *xsurf, uint32_t ser) {
  xdg_surface_ack_configure(xsurf, ser);
  if (!pixl) {
    resz();
  }
  draw();
  wl_surface_attach(surf, bfr, 0, 0);
  wl_surface_damage_buffer(surf, 0, 0, w, h);
  wl_surface_commit(surf);
}
void sh_ping(void *data, struct xdg_wm_base *sh, uint32_t ser) {
  xdg_wm_base_pong(sh, ser);
}
struct xdg_wm_base_listener sh_list = {.ping = sh_ping};

struct xdg_surface_listener xsurf_list = {.configure = xsurf_conf};
void reg_glob(void *data, struct wl_registry *reg, uint32_t name,
              const char *intf, uint32_t v) {
  if (!strcmp(intf, wl_compositor_interface.name)) {
    comp = wl_registry_bind(reg, name, &wl_compositor_interface, 4);
  } else if (!strcmp(intf, wl_shm_interface.name)) {
    shm = wl_registry_bind(reg, name, &wl_shm_interface, 1);
  } else if (!strcmp(intf, xdg_wm_base_interface.name)) {
    sh = wl_registry_bind(reg, name, &xdg_wm_base_interface, 1);
    xdg_wm_base_add_listener(sh, &sh_list, 0);
  }
}
void top_conf(void *data, struct xdg_toplevel *top, int32_t w, int32_t h,
              struct wl_array *s) {}
void top_cls(void *data, struct xdg_toplevel *top) {}
struct xdg_toplevel_listener top_list = {.configure = top_conf};
void reg_glob_rem(void *data, struct wl_registry *reg, uint32_t name) {}

struct wl_registry_listener reg_list = {.global = reg_glob,
                                        .global_remove = reg_glob_rem};

int main() {
  struct wl_display *disp = wl_display_connect(0);
  struct wl_registry *reg = wl_display_get_registry(disp);
  wl_registry_add_listener(reg, &reg_list, 0);
  wl_display_roundtrip(disp);
  surf = wl_compositor_create_surface(comp);
  struct xdg_surface *xsurf = xdg_wm_base_get_xdg_surface(sh, surf);
  xdg_surface_add_listener(xsurf, &xsurf_list, 0);
  top = xdg_surface_get_toplevel(xsurf);
  xdg_toplevel_add_listener(top, &top_list, 0);
  xdg_toplevel_set_title(top, "wayland client window");
  wl_surface_commit(surf);
  while (wl_display_dispatch(disp)) {
  }

  if (bfr) {
    wl_buffer_destroy(bfr);
  }
  wl_surface_destroy(surf);
  wl_display_disconnect(disp);
  return 0;
}
