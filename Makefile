
CC = gcc
CFLAGS = -I. -Wall
LDFLAGS = -lwayland-client -lm -lpthread

SRC = main.c wayland_init.c render.c image.c xdg-shell-protocol.c
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
