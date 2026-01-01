#ifndef INNO_CONFIG_H
#define INNO_CONFIG_H

#include <stdint.h>

typedef struct {
    char font_family[64];
    uint32_t text_color; // ARGB
    uint32_t bg_color;   // ARGB
} AppConfig;

/* Load config from file. Returns 0 on success, -1 on error. */
int load_config(const char *path, AppConfig *out_config);

#endif
