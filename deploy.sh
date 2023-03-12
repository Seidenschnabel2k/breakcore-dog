#!/usr/bin/env sh 

docker build -t breakcore-dog . && docker tag breakcore-dog seidenschnabel2k/breakcore-dog && docker push seidenschnabel2k/breakcore-dog
