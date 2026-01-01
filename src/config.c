#include "config.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static uint32_t parse_color(const char *hex_str) {
    if (hex_str[0] == '#') hex_str++; // Skip '#'
    return (uint32_t)strtoul(hex_str, NULL, 16);
}

int load_config(const char *path, AppConfig *out_config) {
    // Defaults
    strcpy(out_config->font_family, "sans-serif");
    out_config->text_color = 0xFFFFFFFF; // White
    out_config->bg_color = 0x80000000;   // Semi-transparent black

    FILE *f = fopen(path, "r");
    if (!f) return -1;

    char line[256];
    while (fgets(line, sizeof(line), f)) {
        if (line[0] == '#' || line[0] == '\n') continue;
        
        char *key = strtok(line, "=");
        char *val = strtok(NULL, "\n");
        
        if (key && val) {
            // Trim whitespace (simple version)
            while(*key == ' ') key++;
            while(*val == ' ') val++;
            
            if (strcmp(key, "font") == 0) {
                strncpy(out_config->font_family, val, 63);
                out_config->font_family[63] = '\0';
            } else if (strcmp(key, "text_color") == 0) {
                out_config->text_color = parse_color(val);
            } else if (strcmp(key, "bg_color") == 0) {
                out_config->bg_color = parse_color(val);
            }
        }
    }
    fclose(f);
    return 0;
}
