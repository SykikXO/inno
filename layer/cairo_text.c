#include "cairo_text.h"
#include <cairo/cairo.h>
#include <math.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

int render_cairo_text(const char *text, struct CairoText *ct, int *w, int *h, AppConfig *cfg) {
  // Helpers for ARGB
  double a_bg = ((cfg->bg_color >> 24) & 0xFF) / 255.0;
  double r_bg = ((cfg->bg_color >> 16) & 0xFF) / 255.0;
  double g_bg = ((cfg->bg_color >> 8) & 0xFF) / 255.0;
  double b_bg = (cfg->bg_color & 0xFF) / 255.0;

  double a_txt = ((cfg->text_color >> 24) & 0xFF) / 255.0;
  double r_txt = ((cfg->text_color >> 16) & 0xFF) / 255.0;
  double g_txt = ((cfg->text_color >> 8) & 0xFF) / 255.0;
  double b_txt = (cfg->text_color & 0xFF) / 255.0;

  // Step 1: Create dummy
  cairo_surface_t *dummy_surface =
      cairo_image_surface_create(CAIRO_FORMAT_ARGB32, 1, 1);
  cairo_t *cr = cairo_create(dummy_surface);

  // Step 2: Set font
  cairo_select_font_face(cr, cfg->font_family, CAIRO_FONT_SLANT_ITALIC,
                         CAIRO_FONT_WEIGHT_BOLD);
  cairo_set_font_size(cr, 24);

  // Step 3: Measure
  cairo_text_extents_t extents;
  cairo_text_extents(cr, text, &extents);

  // Step 4: Write back dimensions
  *w = (int)ceil(extents.width) + 20; // Padding
  *h = (int)ceil(extents.height) + 20;

  cairo_destroy(cr);
  cairo_surface_destroy(dummy_surface);

  // Step 5: Actual surface
  if (ct->surface) {
      cairo_surface_destroy(ct->surface);
  }
  ct->width = *w;
  ct->height = *h;
  ct->surface =
      cairo_image_surface_create(CAIRO_FORMAT_ARGB32, ct->width, ct->height);
  if (cairo_surface_status(ct->surface) != CAIRO_STATUS_SUCCESS) {
    return -1;
  }

  // Step 6: Draw
  cr = cairo_create(ct->surface);
  
  // Clear background
  cairo_set_source_rgba(cr, 0, 0, 0, 0);
  cairo_paint(cr);
  
  // Fill background (rounded rect optional, but detailed)
  cairo_set_source_rgba(cr, r_bg, g_bg, b_bg, a_bg);
  cairo_rectangle(cr, 0, 0, *w, *h);
  cairo_fill(cr);
  
  // Draw Text
  cairo_set_source_rgba(cr, r_txt, g_txt, b_txt, a_txt);
  cairo_select_font_face(cr, cfg->font_family, CAIRO_FONT_SLANT_ITALIC,
                         CAIRO_FONT_WEIGHT_BOLD);
  cairo_set_font_size(cr, 24);
  cairo_set_antialias(cr, CAIRO_ANTIALIAS_SUBPIXEL);

  // Center text in padded area
  // y_bearing is negative (distance from origin to top of glyphs)
  // height is total height
  // We want to center the text visually.
  double text_y_center = (extents.height / 2.0) + extents.y_bearing;
  double box_center = *h / 2.0;
  cairo_move_to(cr, 10, box_center - text_y_center); 
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
