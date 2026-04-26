#!/bin/bash

aws ec2 describe-instances \
  --filters "Name=tag:Name,Values=game-server" \
  --query 'Reservations[0].Instances[0].InstanceId' \
  --output text
