#include <stdio.h>

#ifdef _WIN32
__declspec(dllexport)
#endif
void greet(const char *name) {
    printf("Hello, %s!\n", name);
}
