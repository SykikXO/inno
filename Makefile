CC = gcc
CFLAGS = -I. -Wall $(shell pkg-config --cflags cairo)
LDFLAGS = -lwayland-client -lm -lpthread $(shell pkg-config --libs cairo)

SRC = main.c wayland_init.c render.c cairo_image.c image.c xdg-shell-protocol.c
OBJ = $(SRC:.c=.o)
TARGET = execthis

.PHONY: all clean

all: $(TARGET)

$(TARGET): $(OBJ)
	$(CC) $(OBJ) -o $@ $(LDFLAGS)

%.o: %.c
	$(CC) $(CFLAGS) -c $< -o $@

clean:
	rm -f $(OBJ) $(TARGET)
