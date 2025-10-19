#include "render.h"
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>

static int allocate_shm_file(size_t size) {
  char name[] = "/tmpwayXXXXXX";
  int fd = shm_open(name, O_RDWR | O_CREAT | O_EXCL, 0600);
  if (fd < 0)
    return -1;
  shm_unlink(name);
  ftruncate(fd, size);
  return fd;
}

int create_buffer(struct wl_shm *shm, struct RenderBuffer *out, uint16_t width,
                  uint16_t height) {
  size_t size = width * height * 4;
  int fd = allocate_shm_file(size);
  out->pixels = mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);

  struct wl_shm_pool *pool = wl_shm_create_pool(shm, fd, size);
  out->buffer = wl_shm_pool_create_buffer(pool, 0, width, height, width * 4,
                                          WL_SHM_FORMAT_ARGB8888);
  wl_shm_pool_destroy(pool);
  close(fd);

  out->width = width;
  out->height = height;
  return 0;
}

void destroy_buffer(struct RenderBuffer *buf) {
  if (buf->buffer)
    wl_buffer_destroy(buf->buffer);
}
