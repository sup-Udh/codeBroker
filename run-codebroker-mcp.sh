#!/bin/bash
cd /home/labuser/code/codeBroker

# Automatically load all variables from the .env file and export them
if [ -f .env ]; then
    export $(grep -v '^#' .env | xargs)
fi

cargo run -p mcp