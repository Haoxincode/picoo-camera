// Apple linker smoke for REQ-PICOO-STACK-003 / REQ-PICOO-STACK-007.
#include "picoo_camera.h"

int main(void) {
  const char *name = picoo_protocol_name();
  return name != NULL && name[0] == 'P' ? 0 : 1;
}
