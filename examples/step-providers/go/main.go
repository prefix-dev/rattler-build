package main

import (
	"fmt"

	"github.com/google/uuid"
)

func main() {
	fmt.Printf("hello from the Go provider (%s)\n", uuid.New())
}
