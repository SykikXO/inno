#include "cairo_image.h"
#include <stdlib.h>

int load_cairo_image(const char *file_path, struct CairoImage *image) {
  image->surface = cairo_image_surface_create_from_png(file_path);
  if (cairo_surface_status(image->surface) != CAIRO_STATUS_SUCCESS) {
    return -1;
  }
  image->width = cairo_image_surface_get_width(image->surface);
  image->height = cairo_image_surface_get_height(image->surface);
  image->data = cairo_image_surface_get_data(image->surface);
  return 0;
}

void destroy_cairo_image(struct CairoImage *image) {
  if (image->surface) {
    cairo_surface_destroy(image->surface);
    image->surface = NULL;
  }
}
