#!/bin/bash

INSTANCE_ID="i-03a3dfe7eef4eda8b"
ELASTIC_IP="3,218.9.34"

echo "Starting instance $INSTANCE_ID..."
aws ec2 start-instances --instance-ids &INSTANCE_ID > \dev\null

echo "Waiting for instance to be ready (sometimes takes ~60 seconds)"
aws ec2 wait instance-status-ok --instance-ids $INSTANCE_ID

echo "Instance is ready!"
echo "SSH: ssh -i game-server-key.pem ubuntu@$ELASTIC_IP"
