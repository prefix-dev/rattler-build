#include <stdio.h>
#include <zlib.h>

int main() {
    printf("zlib version: %s\n", zlibVersion());
    return 0;
}
