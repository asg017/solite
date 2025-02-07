#include "solite-stdlib.h"
int core_init(const char *dummy) {
  return sqlite3_auto_extension((void *)solite_stdlib_init);
}
