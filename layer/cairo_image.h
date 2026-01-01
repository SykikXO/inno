
#ifndef CAIRO_IMAGE_H
#define CAIRO_IMAGE_H

#include <cairo/cairo.h>

struct CairoImage {
  unsigned char *data;
  int width;
  int height;
  cairo_surface_t *surface;
};

int load_cairo_image(const char *file_path, struct CairoImage *image);
void destroy_cairo_image(struct CairoImage *image);

#endif // CAIRO_IMAGE_H
