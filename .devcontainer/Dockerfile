FROM ubuntu:24.04

# TO DO: Adjust the following as needed for your development environment
# - Get host OS user UID/GID and set them as build args to avoid permission issues with mounted volumes

# Avoid interactive prompts during package installation
ENV DEBIAN_FRONTEND=noninteractive

# Install any additional tools you might want
RUN apt-get update

# Install 
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

RUN sudo curl -fsSL https://claude.ai/install.sh | bash
RUN sudo curl -fsSL https://opencode.ai/install | bash
#RUN echo 'export PATH="/home/engemil/.local/bin:$PATH"' >> /home/$USERNAME/.bashrc && source /home/$USERNAME/.bashrc
RUN sudo echo 'export PATH="/home/engemil/.local/bin:$PATH"' >> /home/$USERNAME/.bashrc
# Add Super commando "claude --dangerously-skip-permissions" as alias claude-unchained
RUN sudo echo 'alias claude-unchained="claude --dangerously-skip-permissions"' >> /home/$USERNAME/.bashrc


# Clean up
RUN rm -rf /var/lib/apt-get/lists/*

# Make workspace directory
RUN sudo mkdir /workspace && sudo chown $USERNAME:$USERNAME /workspace
WORKDIR /workspace
