#!/bin/bash
echo "Graph store"
du -sch storage/* storage/

echo "SDK store"
du -sch store/* store/

echo "New SDK store"
du -sch store_new/* store_new/