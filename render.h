
#pragma once
#include <stdint.h>
#include <wayland-client.h>

struct RenderBuffer {
  struct wl_buffer *buffer;
  uint8_t *pixels;
  uint16_t width;
  uint16_t height;
};

int create_buffer(struct wl_shm *shm, struct RenderBuffer *out, uint16_t width,
                  uint16_t height);
void destroy_buffer(struct RenderBuffer *buf);
