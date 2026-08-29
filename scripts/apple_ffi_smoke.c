// Apple linker smoke for REQ-PICOO-STACK-003 / REQ-PICOO-STACK-007.
#include "picoo_camera.h"

int main(void) {
  const char *version = picoo_protocol_version();
  return version != NULL && version[0] == 'P' ? 0 : 1;
}
