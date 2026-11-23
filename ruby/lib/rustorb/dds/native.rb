# frozen_string_literal: true

# The Ruby side of DDS::Native (rustorb_dds_ext): the dispatcher threads
# that run listeners, and the registry of listeners the extension calls by
# id.
#
# Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

module DDS
  module Native
    @listeners = {}
    @next_id = 0
    @lock = Mutex.new
    @dispatcher_lock = Mutex.new

    class << self
      # Starts a dispatcher thread (the extension asks for one more when
      # every dispatcher is busy).
      def spawn_dispatcher
        t = Thread.new { dispatcher_loop }
        t.name = "rustorb-dds-dispatcher" if t.respond_to?(:name=)
        t.report_on_exception = true
        t
      end

      # Starts dispatchers until n are running.
      def ensure_dispatchers(n = 2)
        @dispatcher_lock.synchronize do
          return if dispatchers >= n

          (n - dispatchers).times { spawn_dispatcher }
          Thread.pass until dispatchers >= n
        end
      end

      # Registers a listener for target, which gets its calls
      # (target.deliver(event, endpoint, arg, listener)); returns its id.
      def listener(target, listener)
        ensure_dispatchers
        @lock.synchronize do
          @next_id += 1
          @listeners[@next_id] = [target, listener]
          @next_id
        end
      end

      def forget_listener(id)
        @lock.synchronize { @listeners.delete(id) }
      end

      # Called by the extension, with the GVL, for each listener call:
      # event is the name of the call (or :release), endpoint the GUID of
      # the writer or reader the status is of (or the topic's name), arg
      # the status as an Array (or a subscriber's handle).
      def dispatch(id, event, endpoint, arg)
        if event == :release
          forget_listener(id)
          return
        end
        target, listener = @lock.synchronize { @listeners[id] }
        if target
          target.deliver(event, endpoint, arg, listener)
        elsif arg.is_a?(Handle)
          release(arg)
        end
      rescue Exception => e # rubocop:disable Lint/RescueException
        warn "rustorb-dds: the listener's #{event} raised #{e.class}: #{e.message}"
      end
    end

    # The target of the listener a writer or reader is made with: its calls
    # wait until the writer or reader is there.
    class Slot
      def initialize
        @lock = Mutex.new
        @entity = nil
        @pending = []
      end

      def entity=(entity)
        pending = @lock.synchronize do
          @entity = entity
          p = @pending
          @pending = []
          p
        end
        pending.each { |args| entity.deliver(*args) }
      end

      def deliver(*args)
        entity = @lock.synchronize do
          @pending << args unless @entity
          @entity
        end
        entity&.deliver(*args)
      end
    end

    # Writers and readers by GUID, for the listeners of their participant,
    # publisher, subscriber: a status of one not yet known waits (up to 10
    # s) until it is.
    class Endpoints
      def initialize
        @lock = Mutex.new
        @by_guid = {}
        @pending = {}
      end

      def add(guid, entity)
        pending = @lock.synchronize do
          @by_guid[guid] = entity
          @pending.delete(guid) || []
        end
        pending.each do |_, blk|
          blk.call(entity)
        rescue StandardError => e
          warn "rustorb-dds: a listener raised #{e.class}: #{e.message}"
        end
      end

      def remove(guid)
        @lock.synchronize { @by_guid.delete(guid) }
      end

      def deliver(guid, &blk)
        entity = @lock.synchronize do
          e = @by_guid[guid]
          unless e
            now = Process.clock_gettime(Process::CLOCK_MONOTONIC)
            @pending.each_value { |q| q.reject! { |t, _| now - t > 10 } }
            @pending.delete_if { |_, q| q.empty? }
            (@pending[guid] ||= []) << [now, blk]
          end
          e
        end
        blk.call(entity) if entity
      end
    end
  end
end
