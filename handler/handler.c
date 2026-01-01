#include <stdio.h>
#include <stdlib.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>

#define VBUS_PATH "/sys/class/power_supply/ADP1/online"
// Returns 1 if connected, 0 if disconnected, -1 on error
int is_charger_connected() {
  FILE *f = fopen(VBUS_PATH, "r");
  if (!f) {
    perror("Failed to open charger status file");
    return -1;
  }
  char buf[4];
  if (!fgets(buf, sizeof(buf), f)) {
    perror("Failed to read charger status");
    fclose(f);
    return -1;
  }
  fclose(f);
  return (buf[0] == '1') ? 1 : 0;
}

void run_in_background() {
  pid_t pid, sid;

  // Fork off the parent process
  pid = fork();
  if (pid < 0) {
    perror("fork failed");
    exit(EXIT_FAILURE);
  }
  if (pid > 0) {
    // Parent exits
    exit(EXIT_SUCCESS);
  }

  // Child continues

  // Create new session and process group
  sid = setsid();
  if (sid < 0) {
    perror("setsid failed");
    exit(EXIT_FAILURE);
  }

  // Change working directory
  if ((chdir("/")) < 0) {
    perror("chdir failed");
    exit(EXIT_FAILURE);
  }

  // Redirect standard files to /dev/null
  freopen("/dev/null", "r", stdin);
  freopen("/dev/null", "w", stdout);
  freopen("/dev/null", "w", stderr);

  // Daemon is now detached from terminal
}

int main() {
  int state = is_charger_connected();
  run_in_background();

  while (1) {
    int connected = is_charger_connected();
    if (connected == 1 && state != connected) {
      system("timeout 3s /home/sykik/Dev/dum/execthis -c");
    } else if (connected == 0 && state != connected) {
      system("timeout 3s /home/sykik/Dev/dum/execthis -d");
    }
    state = connected;
    sleep(1); // Check every 5 seconds
  }

  return 0;
}
