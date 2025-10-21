#include "cairo_text.h"
#include <stdlib.h>
#include <string.h>

int render_cairo_text(const char *text, struct CairoText *ct) {
  ct->width = 800;
  ct->height = 600;

  // Create an ARGB Cairo surface for drawing
  ct->surface =
      cairo_image_surface_create(CAIRO_FORMAT_ARGB32, ct->width, ct->height);
  if (cairo_surface_status(ct->surface) != CAIRO_STATUS_SUCCESS) {
    return -1;
  }

  cairo_t *cr = cairo_create(ct->surface);

  // Background color (light grey)
  cairo_set_source_rgb(cr, 0.9, 0.9, 0.9);
  cairo_paint(cr);

  // Set text properties
  cairo_set_source_rgb(cr, 0.1, 0.2, 0.6); // text color
  cairo_select_font_face(cr, "Sans", CAIRO_FONT_SLANT_NORMAL,
                         CAIRO_FONT_WEIGHT_BOLD);
  cairo_set_font_size(cr, 48);

  // Move and draw text
  cairo_move_to(cr, 100, 300);
  cairo_show_text(cr, text);

  cairo_destroy(cr);

  ct->data = cairo_image_surface_get_data(ct->surface);

  return 0;
}

void destroy_cairo_text(struct CairoText *ct) {
  if (ct->surface) {
    cairo_surface_destroy(ct->surface);
    ct->surface = NULL;
  }
}
