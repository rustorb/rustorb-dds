// A small test harness: CHECK, CHECK_THROWS and the test registry.
#ifndef RUSTORB_DDS_TESTS_CHECK_HPP
#define RUSTORB_DDS_TESTS_CHECK_HPP

#include <exception>
#include <functional>
#include <iostream>
#include <string>
#include <utility>
#include <vector>

namespace check {

struct failure {
  std::string what;
};

inline std::vector<std::pair<const char*, std::function<void()>>>& tests() {
  static std::vector<std::pair<const char*, std::function<void()>>> t;
  return t;
}

struct registrar {
  registrar(const char* name, std::function<void()> f) { tests().emplace_back(name, std::move(f)); }
};

inline int run_all() {
  int failed = 0;
  for (auto& t : tests()) {
    try {
      t.second();
      std::cout << "ok   " << t.first << std::endl;
    } catch (const failure& f) {
      std::cout << "FAIL " << t.first << ": " << f.what << std::endl;
      failed++;
    } catch (const std::exception& e) {
      std::cout << "FAIL " << t.first << ": exception: " << e.what() << std::endl;
      failed++;
    }
  }
  std::cout << tests().size() - failed << " passed, " << failed << " failed" << std::endl;
  return failed ? 1 : 0;
}

}  // namespace check

#define CHECK_CAT2(a, b) a##b
#define CHECK_CAT(a, b) CHECK_CAT2(a, b)
#define TEST(name)                                                         \
  static void name();                                                      \
  static check::registrar CHECK_CAT(registrar_, name)(#name, &name);       \
  static void name()

#define CHECK(cond)                                                                    \
  do {                                                                                 \
    if (!(cond))                                                                       \
      throw check::failure{std::string(__FILE__) + ":" + std::to_string(__LINE__) +    \
                           ": " #cond};                                                \
  } while (0)

#define CHECK_THROWS(expr, type)                                                       \
  do {                                                                                 \
    bool thrown = false;                                                               \
    try {                                                                              \
      expr;                                                                            \
    } catch (const type&) {                                                            \
      thrown = true;                                                                   \
    }                                                                                  \
    if (!thrown)                                                                       \
      throw check::failure{std::string(__FILE__) + ":" + std::to_string(__LINE__) +    \
                           ": " #expr " does not throw " #type};                       \
  } while (0)

#endif
