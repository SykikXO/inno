#include "layerhandler.h"
#include "cairo_text.h"
#include "render.h"
#include "wayland_init.h"
#include "wlr-layer-shell-unstable-v1-client-protocol.h"
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>
#include <wayland-client-protocol.h>

// Globals (Internal to this module)
struct zwlr_layer_shell_v1 *layer_shell = NULL;
struct WaylandState wl = {0};
struct wl_surface *surface;
struct RenderBuffer render_buffer = {0};
static struct ZwlrLayerSurfaceV1 *layer_surface = NULL; // Fixed type name capitalization if needed, but standard is lower underscore usually. Protocol defines it.
static struct zwlr_layer_surface_v1 *wlr_layer_surface = NULL;

static struct CairoText cairo_text;
static uint32_t current_width = 400;
static uint32_t current_height = 300;
static int text_w, text_h;

// --- Forward Declarations ---
static void registry_handle_global(void *data, struct wl_registry *registry, uint32_t id, const char *interface, uint32_t version);
static void layer_surface_configure(void *data, struct zwlr_layer_surface_v1 *surface, uint32_t serial, uint32_t w, uint32_t h);
static void layer_surface_closed(void *data, struct zwlr_layer_surface_v1 *surface);

// --- Listeners ---
static const struct wl_registry_listener registry_listener = {
    .global = registry_handle_global,
    .global_remove = NULL,
};

static const struct zwlr_layer_surface_v1_listener layer_surface_listener = {
    .configure = layer_surface_configure,
    .closed = layer_surface_closed,
};

// --- Implementation ---

static void registry_handle_global(void *data, struct wl_registry *registry, uint32_t id, const char *interface, uint32_t version) {
    struct WaylandState *state = data;
    if (strcmp(interface, zwlr_layer_shell_v1_interface.name) == 0) {
        layer_shell = wl_registry_bind(registry, id, &zwlr_layer_shell_v1_interface, 1);
    } else if (strcmp(interface, wl_compositor_interface.name) == 0) {
        state->compositor = wl_registry_bind(registry, id, &wl_compositor_interface, version);
    } else if (strcmp(interface, wl_shm_interface.name) == 0) {
        state->shm = wl_registry_bind(registry, id, &wl_shm_interface, version);
    }
}

static int configured = 0;

static void layer_surface_configure(void *data, struct zwlr_layer_surface_v1 *surface,
                                    uint32_t serial, uint32_t w, uint32_t h) {
    zwlr_layer_surface_v1_ack_configure(surface, serial);
    
    if (w > 0 && h > 0) {
        current_width = w;
        current_height = h;
    }
    configured = 1;
}

static void layer_surface_closed(void *data, struct zwlr_layer_surface_v1 *surface) {
    wlr_layer_surface = NULL;
    configured = 0;
}

static AppConfig layer_config;

int layer_init(AppConfig *cfg) {
    if (cfg) {
        memcpy(&layer_config, cfg, sizeof(AppConfig));
    } else {
        // Fallback defaults
        strcpy(layer_config.font_family, "sans-serif");
        layer_config.text_color = 0xFFFFFFFF;
        layer_config.bg_color = 0x80000000;
    }

    wl.display = wl_display_connect(NULL);
    if (!wl.display) {
        fprintf(stderr, "Failed to connect to Wayland display\n");
        return -1;
    }

    wl.registry = wl_display_get_registry(wl.display);
    wl_registry_add_listener(wl.registry, &registry_listener, &wl);
    wl_display_roundtrip(wl.display);

    if (!layer_shell || !wl.compositor || !wl.shm) {
        fprintf(stderr, "Missing required globals (Compositor, SHM, or LayerShell)\n");
        return -1;
    }

    surface = wl_compositor_create_surface(wl.compositor);
    if (!surface) {
        fprintf(stderr, "Failed to create surface\n");
        return -1;
    }
    
    // Create the layer surface initially
    wlr_layer_surface = zwlr_layer_shell_v1_get_layer_surface(
        layer_shell, surface, NULL, ZWLR_LAYER_SHELL_V1_LAYER_OVERLAY, "inno_notification");
    
    zwlr_layer_surface_v1_add_listener(wlr_layer_surface, &layer_surface_listener, NULL);
    
    // Default anchor configuration (Bottom Right)
    zwlr_layer_surface_v1_set_anchor(wlr_layer_surface,
                                     ZWLR_LAYER_SURFACE_V1_ANCHOR_BOTTOM |
                                     ZWLR_LAYER_SURFACE_V1_ANCHOR_RIGHT);
    zwlr_layer_surface_v1_set_margin(wlr_layer_surface, 10, 10, 10, 10);
    zwlr_layer_surface_v1_set_keyboard_interactivity(wlr_layer_surface, 0); // No keyboard focus
    zwlr_layer_surface_v1_set_size(wlr_layer_surface, 1, 1);
    
    wl_surface_commit(surface);
    
    // Wait for initial configure
    while(!configured) {
        if (wl_display_dispatch(wl.display) == -1) {
            fprintf(stderr, "Wayland disconnect during init\n");
            return -1;
        }
    }
    
    return 0;
}

void layer_show_text(const char *text) {
    if (!wlr_layer_surface || !surface || !configured) return;

    // Render text to cairo surface
    if (render_cairo_text(text, &cairo_text, &text_w, &text_h, &layer_config) != 0) {
        fprintf(stderr, "Failed to render text\n");
        return;
    }
    
    // Verify we have a valid buffer size before rendering
    if (text_w <= 0 || text_h <= 0) return;

    // Resize buffer if needed
    if (render_buffer.buffer) destroy_buffer(&render_buffer);
    create_buffer(wl.shm, &render_buffer, text_w, text_h);
    
    if (render_buffer.pixels == MAP_FAILED) {
        perror("mmap failed");
        return;
    }
    
    // Copy pixels
    memcpy(render_buffer.pixels, cairo_text.data, text_w * text_h * 4);
    
    // Resize the layer surface to match content
    zwlr_layer_surface_v1_set_size(wlr_layer_surface, text_w, text_h);
    
    // Commit to request configuration change
    wl_surface_commit(surface);
    
    // Attach and commit
    wl_surface_attach(surface, render_buffer.buffer, 0, 0);
    wl_surface_damage(surface, 0, 0, text_w, text_h);
    wl_surface_commit(surface);
    
    wl_display_flush(wl.display);
}

void layer_hide() {
    if (!wlr_layer_surface || !surface) return;
    
    // Instead of unmapping (which resets config), make it 1x1 transparent
    if (render_buffer.buffer) destroy_buffer(&render_buffer);
    create_buffer(wl.shm, &render_buffer, 1, 1); // 1x1
    
    if (render_buffer.pixels && render_buffer.pixels != MAP_FAILED) {
        memset(render_buffer.pixels, 0, 4); // Clear to 0 (Transparent)
    }

    zwlr_layer_surface_v1_set_size(wlr_layer_surface, 1, 1);
    
    wl_surface_attach(surface, render_buffer.buffer, 0, 0);
    wl_surface_damage(surface, 0, 0, 1, 1);
    wl_surface_commit(surface);
    wl_display_flush(wl.display);
}

int layer_dispatch() {
    if (wl_display_dispatch(wl.display) == -1) return -1;
    return 0;
}

int layer_get_fd() {
    return wl_display_get_fd(wl.display);
}

void layer_cleanup() {
    destroy_cairo_text(&cairo_text);
    destroy_buffer(&render_buffer);
    if (wlr_layer_surface) zwlr_layer_surface_v1_destroy(wlr_layer_surface);
    if (surface) wl_surface_destroy(surface);
    if (layer_shell) zwlr_layer_shell_v1_destroy(layer_shell);
    wayland_cleanup(&wl);
}
