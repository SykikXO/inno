#define STB_IMAGE_IMPLEMENTATION
#include <stb/stb_image.h>
#include <stdint.h>
#include <stdio.h>

int load_image(const char *path, uint8_t *dest, int dest_w, int dest_h) {
  int iw, ih, channels;
  unsigned char *img = stbi_load(path, &iw, &ih, &channels, 4);
  if (!img) {
    fprintf(stderr, "Failed to load image: %s\n", path);
    return -1;
  }

  for (int y = 0; y < dest_h && y < ih; y++) {
    for (int x = 0; x < dest_w && x < iw; x++) {
      uint8_t *s = img + 4 * (y * iw + x);
      uint8_t *d = dest + 4 * (y * dest_w + x);
      d[0] = s[2];
      d[1] = s[1];
      d[2] = s[0];
      d[3] = s[3];
    }
  }

  stbi_image_free(img);
  return 0;
}
