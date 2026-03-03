#ifdef _WIN32
__declspec(dllimport)
#endif
void greet(const char *name);

int main(void) {
    greet("world");
    return 0;
}
