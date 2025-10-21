#ifndef CAIRO_TEXT_H
#define CAIRO_TEXT_H

#include <cairo/cairo.h>

struct CairoText {
  unsigned char *data;
  int width;
  int height;
  cairo_surface_t *surface;
};

int render_cairo_text(const char *text, struct CairoText *ct, int *w, int *h);
void destroy_cairo_text(struct CairoText *ct);

#endif
