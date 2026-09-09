# GGML Vulkan probe

`uta-ggml-vulkan-probe` is a read-only development utility retained for future
GGML tuning on Intel Xe and other Vulkan devices. It loads the Vulkan loader,
creates an instance, and reports physical-device, subgroup, queue, memory, and
extension properties as JSON. It does not create a logical device or execute an
inference graph.

This directory is deliberately outside the product inference runtime: Uta!
Studio model execution is owned by GGML workers only.
