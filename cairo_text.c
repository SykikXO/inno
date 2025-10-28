#include "cairo_text.h"
#include <cairo/cairo.h>
#include <math.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

int render_cairo_text(const char *text, struct CairoText *ct, int *w, int *h) {
  // Step 1: Create dummy surface and context for measuring text
  cairo_surface_t *dummy_surface =
      cairo_image_surface_create(CAIRO_FORMAT_ARGB32, 1, 1);
  cairo_t *cr = cairo_create(dummy_surface);

  // Step 2: Set font properties (same as for actual rendering)
  cairo_select_font_face(cr, "Iosevka NFM", CAIRO_FONT_SLANT_ITALIC,
                         CAIRO_FONT_WEIGHT_BOLD);
  cairo_set_font_size(cr, 24);

  // Step 3: Measure text extents
  cairo_text_extents_t extents;
  cairo_text_extents(cr, text, &extents);

  // Step 4: Write measured width and height back to *w and *h
  *w = (int)ceil(extents.width);
  *h = (int)ceil(extents.height);

  cairo_destroy(cr);
  cairo_surface_destroy(dummy_surface);

  // Step 5: Now create actual surface of the required size
  ct->width = *w;
  ct->height = *h;
  ct->surface =
      cairo_image_surface_create(CAIRO_FORMAT_ARGB32, ct->width, ct->height);
  if (cairo_surface_status(ct->surface) != CAIRO_STATUS_SUCCESS) {
    return -1;
  }

  // Step 6: Create context & draw

  cr = cairo_create(ct->surface);
  cairo_set_source_rgb(cr, 0.2, 0.2, 0.2);
  cairo_paint(cr);
  cairo_set_source_rgb(cr, 0.3, 0.9, 0.6);
  cairo_select_font_face(cr, "Iosevka NFM", CAIRO_FONT_SLANT_ITALIC,
                         CAIRO_FONT_WEIGHT_BOLD);
  cairo_set_font_size(cr, 24);
  cairo_set_antialias(cr, CAIRO_ANTIALIAS_SUBPIXEL);

  // Adjust y position by -y_bearing to render at baseline
  cairo_move_to(cr, 0, -extents.y_bearing);
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
