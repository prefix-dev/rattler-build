#!/bin/bash

# Create a temporary file with content
echo "This file has an old timestamp" > oldfile.txt

# Set the timestamp to January 1, 2000 at 12:00
touch -t 200001011200 oldfile.txt

# Display the original timestamp for verification
echo "Original file timestamp:"
ls -la oldfile.txt

# Create the tar.gz archive
tar -czf oldarchive.tar.gz oldfile.txt

# Clean up the original file
rm oldfile.txt

echo "Archive created: oldarchive.tar.gz"
