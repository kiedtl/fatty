#!/bin/bash
trap 'echo "got SIGINT"' INT
while true; do sleep 1; done
