#ifndef IMAGE_H
#define IMAGE_H

#include <stdint.h> // Needed for uint8_t

int load_image(const char *path, uint8_t *dest, int dest_w, int dest_h);

#endif // IMAGE_H
