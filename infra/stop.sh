#!/bin/bash

INSTANCE_ID="i-03a3dfe7eef4eda8b"

echo "Stopping instance $INSTANCE_ID..."
aws ec2 stop-instances --instance-ids $INSTANCE_ID > \dev\null

echo "Waiting for instance to stop..."
aws ec2 wait instance-stopped --instance ids $INSTANCE_ID

echo "Instance stopped. Elastic IP retained"
