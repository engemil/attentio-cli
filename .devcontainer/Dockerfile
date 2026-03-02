FROM ubuntu:24.04

# TO DO: Adjust the following as needed for your development environment
# - Get host OS user UID/GID and set them as build args to avoid permission issues with mounted volumes

# Avoid interactive prompts during package installation
ENV DEBIAN_FRONTEND=noninteractive

# Update and Install dependencies
RUN apt-get update
RUN apt-get install build-essential -y
RUN apt-get install pkg-config -y
RUN apt-get install libudev-dev -y
RUN apt-get install libusb-1.0-0-dev -y

# Install any additional tools you might want
RUN apt-get install git -y
RUN apt-get install curl -y
RUN apt-get install minicom -y
#RUN apt-get install stlink-tools -y
#RUN apt-get install openocd -y
RUN apt-get install dfu-util -y
RUN apt-get install usbutils -y 
RUN apt-get install sudo -y

# If you have permission issues due to running a root in container, you can run the
# following command inside the container: sudo chown -R $(whoami):$(whoami) /workspace

# Prepare User
ARG USERNAME=engemil

# Remove default ubuntu user if exists, to clear UID/GID conflicts
RUN userdel -r ubuntu || true
# Create the group first to avoid "group already exists" error
RUN groupadd $USERNAME
# Create user 'engemil' with home directory and bash shell
RUN useradd -m -s /bin/bash -g $USERNAME $USERNAME
# Add user to sudo group
RUN usermod -aG sudo $USERNAME
# Allow passwordless sudo (typical for dev containers)
RUN echo "engemil ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers

# RUN AS USER FROM HERE
USER $USERNAME

# Install OpenCode CLI
RUN sudo curl -fsSL https://opencode.ai/install | bash

# Install Rust toolchain
RUN sudo curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
#RUN sudo echo 'source $HOME/.cargo/env' >> /home/$USERNAME/.bashrc
RUN echo 'source $HOME/.cargo/env' >> /home/$USERNAME/.bashrc


# Clean up
RUN rm -rf /var/lib/apt-get/lists/*

# Make workspace directory
RUN sudo mkdir /workspace && sudo chown $USERNAME:$USERNAME /workspace
WORKDIR /workspace
