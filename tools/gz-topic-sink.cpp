#include <gz/transport/Node.hh>

#include <atomic>
#include <chrono>
#include <cstddef>
#include <csignal>
#include <cstdint>
#include <iostream>
#include <string>
#include <thread>

namespace
{
volatile std::sig_atomic_t running = 1;

void stop(int)
{
  running = 0;
}
}  // namespace

int main(int argc, char **argv)
{
  if (argc != 2 || std::string(argv[1]) == "--help")
  {
    std::cerr << "usage: gz-topic-sink TOPIC\n";
    return argc == 2 ? 0 : 2;
  }

  std::signal(SIGINT, stop);
  std::signal(SIGTERM, stop);

  const std::string topic = argv[1];
  std::atomic<std::uint64_t> message_count{0};
  std::atomic<std::uint64_t> byte_count{0};
  gz::transport::Node node;
  const bool subscribed = node.SubscribeRaw(
      topic,
      [&message_count, &byte_count](const char *, const std::size_t size,
                                    const gz::transport::MessageInfo &)
      {
        message_count.fetch_add(1, std::memory_order_relaxed);
        byte_count.fetch_add(size, std::memory_order_relaxed);
      });
  if (!subscribed)
  {
    std::cerr << "could not subscribe to " << topic << '\n';
    return 1;
  }

  while (running)
  {
    std::this_thread::sleep_for(std::chrono::milliseconds(100));
  }

  std::cerr << "received " << message_count.load(std::memory_order_relaxed)
            << " message(s), " << byte_count.load(std::memory_order_relaxed)
            << " byte(s) from " << topic << '\n';
  return 0;
}
