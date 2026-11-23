# frozen_string_literal: true

# DDS types: how rustorb-idl -bruby -Wbdds / -Wbdds_only describes them
# (DDS::Types), the base classes of the generated structs and unions
# (DDS::Struct, DDS::Union) and long double (DDS::LongDouble). A type
# builds its DDS-XTypes type with rustorb-dds's type builders and writes
# and reads its values through the sample streams of the C ABI.
#
# Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

require "monitor"

module DDS
  # long double: the bits of an IEEE 754 binary128 (hi, lo), made from and
  # turned into a Float.
  class LongDouble
    attr_reader :hi, :lo

    MASK64 = (1 << 64) - 1

    def initialize(value = 0.0)
      @hi, @lo = LongDouble.bits_of(value.to_f)
    end

    def self.from_bits(hi, lo)
      v = allocate
      v.instance_variable_set(:@hi, hi)
      v.instance_variable_set(:@lo, lo)
      v
    end

    # The binary128 bits of a Float (exactly: every double is a binary128).
    def self.bits_of(f)
      bits = [f].pack("G").unpack1("Q>")
      sign = bits >> 63
      exp = (bits >> 52) & 0x7FF
      mant = bits & ((1 << 52) - 1)
      if exp == 0x7FF
        e = 0x7FFF
        m = mant << 60
      elsif exp.zero?
        if mant.zero?
          e = 0
          m = 0
        else
          shift = mant.bit_length - 1
          e = -1074 + shift + 16_383
          m = (mant - (1 << shift)) << (112 - shift)
        end
      else
        e = exp - 1023 + 16_383
        m = mant << 60
      end
      [(sign << 63) | (e << 48) | (m >> 64), m & MASK64]
    end

    # The nearest Float.
    def to_f
      sign = @hi >> 63
      e = (@hi >> 48) & 0x7FFF
      m = ((@hi & ((1 << 48) - 1)) << 64) | @lo
      v = if e == 0x7FFF
            m.zero? ? Float::INFINITY : Float::NAN
          elsif e.zero?
            Math.ldexp(m.to_f, -16_382 - 112)
          else
            Math.ldexp(((1 << 112) | m).to_f, e - 16_383 - 112)
          end
      sign == 1 ? -v : v
    end

    def ==(other)
      if other.respond_to?(:hi) && other.respond_to?(:lo)
        other.hi == @hi && other.lo == @lo
      else
        other.is_a?(Numeric) && to_f == other
      end
    end
    alias eql? ==

    def hash
      [@hi, @lo].hash
    end

    def inspect
      "#<DDS::LongDouble #{to_f}>"
    end
    alias to_s inspect
  end

  # A struct of rustorb-idl -bruby -Wbdds_only: attr_accessor members
  # (MEMBERS, in IDL order), a positional new, and DEFAULTS, the values of
  # the members new is not given (a Proc is called for a new value).
  class Struct
    MEMBERS = [].freeze
    DEFAULTS = {}.freeze

    def initialize(*args)
      members = self.class::MEMBERS
      raise ArgumentError, "#{self.class}: #{args.size} values for #{members.size} members" if args.size > members.size

      defaults = self.class::DEFAULTS
      members.each_with_index do |m, i|
        v = if args.size > i
              args[i]
            else
              d = defaults[m]
              d.is_a?(Proc) ? d.call : d
            end
        instance_variable_set("@#{m}", v)
      end
    end

    def _values
      self.class::MEMBERS.map { |m| instance_variable_get("@#{m}") }
    end

    def ==(other)
      other.class == self.class && other._values == _values
    end
    alias eql? ==

    def hash
      [self.class, _values].hash
    end

    def inspect
      "#<#{self.class.name} #{self.class::MEMBERS.zip(_values).map { |m, v| "#{m}=#{v.inspect}" }.join(', ')}>"
    end
    alias to_s inspect
  end

  # A union of rustorb-idl -bruby -Wbdds_only: the discriminator (_disc),
  # the value (_value) and an accessor per member, which reads the value if
  # the member is selected (else DDS::Core::PreconditionNotMetError) or
  # selects the member with its first label. A new union has the
  # discriminator a value no label has, which selects the default member
  # (with its zero value) or none.
  class Union
    DEFAULTS = {}.freeze

    attr_reader :_disc, :_value

    def initialize
      _default
    end

    def _set(disc, value)
      @_disc = disc
      @_value = value
      self
    end

    # The IDL name of the selected member (nil for none).
    def _member
      m = self.class._dds_type.member_for(@_disc)
      m&.name
    end

    # Sets the discriminator to another label of the selected member.
    def _disc=(disc)
      t = self.class._dds_type
      now = t.member_for(@_disc)
      unless t.member_for(disc).equal?(now)
        raise DDS::Core::InvalidArgumentError, "#{disc.inspect} does not select #{now&.name || 'no member'}"
      end

      @_disc = disc
    end

    # Selects no member, or the default member: the discriminator a value
    # no label has (or disc).
    def _default(disc = nil)
      t = self.class._dds_type
      @_disc = disc.nil? ? t.implicit_default : disc
      m = t.member_for(@_disc)
      d = m && self.class::DEFAULTS[m.name]
      @_value = d.is_a?(Proc) ? d.call : d
      self
    end

    def _get(name)
      raise DDS::Core::PreconditionNotMetError, "#{self.class.name}: #{name} is not selected" unless _member == name

      @_value
    end

    # Selects the member name with value; disc must be one of its labels.
    def _put(name, value, disc = nil)
      t = self.class._dds_type
      m = t.members.find { |x| x.name == name } or raise ArgumentError, "#{self.class.name} has no member #{name}"
      if disc.nil?
        disc = m.labels.empty? ? t.implicit_default : t.disc_of(m.labels.first)
      elsif !t.member_for(disc).equal?(m)
        raise DDS::Core::InvalidArgumentError, "#{disc.inspect} is not a label of #{name}"
      end
      @_disc = disc
      @_value = value
      self
    end

    def ==(other)
      other.class == self.class && other._disc == @_disc && other._value == @_value
    end
    alias eql? ==

    def hash
      [self.class, @_disc, @_value].hash
    end

    def inspect
      "#<#{self.class.name} _disc=#{@_disc.inspect} #{_member}=#{@_value.inspect}>"
    end
    alias to_s inspect
  end

  # The description of the types (what rustorb-idl writes) and the values'
  # writing and reading.
  module Types
    # Must match the GENERATED_CODE_VERSION of rustorb-idl's ruby_dds.py.
    GENERATED_CODE_VERSION = 1

    # Flags of a struct or union.
    NESTED = 1
    AUTOID_HASH = 2
    # Flags of a member.
    KEY = 1
    OPTIONAL = 2
    MUST_UNDERSTAND = 4
    EXTERNAL = 8
    DEFAULT_LABEL = 64

    LOCK = Monitor.new

    def self.check_generated_code(version, idl)
      return if version == GENERATED_CODE_VERSION

      raise LoadError, "#{idl}: generated for rustorb-dds's generated code version #{version}, " \
                       "this gem has #{GENERATED_CODE_VERSION}: run rustorb-idl again"
    end

    # The type of owner (a class or module), built by the block once.
    def self.lazy(owner)
      t = owner.instance_variable_get(:@_dds_type)
      return t if t

      LOCK.synchronize do
        owner.instance_variable_get(:@_dds_type) || owner.instance_variable_set(:@_dds_type, yield)
      end
    end

    def self.invalid(why)
      raise DDS::Core::InvalidArgumentError, why
    end

    # A type: its DDS-XTypes type (native, built by rustorb-dds) and the
    # writing and reading of its values.
    class Type
      attr_reader :name

      # The 14-octet equivalence hashes of the minimal and complete
      # TypeObjects (DDS-XTypes 1.3, 7.3.4.9).
      def hashes
        Native.type_hashes(native)
      end

      # The type with typedefs followed.
      def resolved
        self
      end

      # The kind of the arrays of the C ABI this type's elements go in, or nil.
      def bulk_kind
        nil
      end
    end

    # A primitive type.
    class Primitive < Type
      attr_reader :kind

      def initialize(kind, name, out, inp)
        super()
        @kind = kind
        @name = name
        @out = out
        @in = inp
      end

      def native
        @native ||= Native.type_primitive(@kind)
      end

      def bulk_kind
        @kind == 0x0B ? nil : @kind
      end

      def write(out, v)
        case @kind
        when 0x0B # float128
          v = LongDouble.new(v) if v.is_a?(Numeric)
          Types.invalid("a long double expected, got #{v.inspect}") unless v.respond_to?(:hi) && v.respond_to?(:lo)
          out.float128(v.hi, v.lo)
        else
          out.public_send(@out, v)
        end
      end

      def read(inp)
        case @kind
        when 0x0B then LongDouble.from_bits(*inp.float128)
        when 0x10 then inp.char8.chr(Encoding::UTF_8)
        else inp.public_send(@in)
        end
      end

      # Elements read in bulk, as values.
      def from_bulk(a)
        @kind == 0x10 ? a.map { |c| c.chr(Encoding::UTF_8) } : a
      end
    end

    BOOLEAN = Primitive.new(0x01, "boolean", :boolean, :boolean)
    BYTE = Primitive.new(0x02, "octet", :octet, :octet)
    INT16 = Primitive.new(0x03, "int16", :int16, :int16)
    INT32 = Primitive.new(0x04, "int32", :int32, :int32)
    INT64 = Primitive.new(0x05, "int64", :int64, :int64)
    UINT16 = Primitive.new(0x06, "uint16", :uint16, :uint16)
    UINT32 = Primitive.new(0x07, "uint32", :uint32, :uint32)
    UINT64 = Primitive.new(0x08, "uint64", :uint64, :uint64)
    FLOAT32 = Primitive.new(0x09, "float32", :float32, :float32)
    FLOAT64 = Primitive.new(0x0A, "float64", :float64, :float64)
    FLOAT128 = Primitive.new(0x0B, "float128", nil, nil)
    INT8 = Primitive.new(0x0C, "int8", :int8, :int8)
    UINT8 = Primitive.new(0x0D, "uint8", :uint8, :uint8)
    CHAR8 = Primitive.new(0x10, "char8", :char8, :char8)
    CHAR16 = Primitive.new(0x11, "char16", :char16, :char16)

    # string<bound> / wstring<bound> (bound 0: unbounded). A wstring is an
    # Array of UTF-16 code units (a String is written as its UTF-16).
    class StringType < Type
      attr_reader :bound

      def initialize(wide, bound)
        super()
        @wide = wide
        @bound = bound
        @name = "#{wide ? 'w' : ''}string#{bound.positive? ? "<#{bound}>" : ''}"
      end

      def native
        @native ||= Native.type_string(@wide, @bound)
      end

      def write(out, v)
        if @wide
          v = v.encode(Encoding::UTF_16LE).unpack("v*") if v.is_a?(String)
          out.wstring(v)
        else
          out.string(v)
        end
      end

      def read(inp)
        @wide ? inp.wstring : inp.string
      end
    end

    def self.string(bound = 0)
      StringType.new(false, bound)
    end

    def self.wstring(bound = 0)
      StringType.new(true, bound)
    end

    # sequence<element, bound>: an Array (octets also as a String, chars as
    # a String).
    class SequenceType < Type
      attr_reader :element, :bound

      def initialize(element, bound)
        super()
        @element = element
        @bound = bound
        @bulk = element.resolved.bulk_kind
        @name = "sequence<#{element.name}#{bound.positive? ? ", #{bound}" : ''}>"
      end

      def native
        @native ||= Native.type_sequence(@element.native, @bound)
      end

      def write(out, v)
        v = v.chars if v.is_a?(String) && @bulk == 0x10
        if v.is_a?(String) && @bulk == 0x02
          out.length(v.bytesize)
          out.array(@bulk, v)
          return
        end
        Types.invalid("an Array expected for a #{@name}, got #{v.inspect}") unless v.is_a?(Array)
        out.length(v.size)
        if @bulk
          out.array(@bulk, v)
        else
          v.each { |e| @element.write(out, e) }
        end
      end

      def read(inp)
        n = inp.length
        if @bulk
          @element.resolved.from_bulk(inp.array(@bulk, n))
        else
          Array.new(n) { @element.read(inp) }
        end
      end
    end

    def self.sequence(element, bound = 0)
      SequenceType.new(element, bound)
    end

    # An array of one or more dimensions: nested Arrays.
    class ArrayType < Type
      attr_reader :element, :dims

      def initialize(element, dims)
        super()
        @element = element
        @dims = dims.dup.freeze
        @bulk = element.resolved.bulk_kind
        @name = "#{element.name}#{dims.map { |d| "[#{d}]" }.join}"
      end

      def native
        @native ||= Native.type_array(@element.native, @dims)
      end

      def write(out, v)
        if @bulk
          flat = []
          flatten(v, 0, flat)
          out.array(@bulk, flat)
        else
          each_element(v, 0) { |e| @element.write(out, e) }
        end
      end

      def read(inp)
        if @bulk
          total = @dims.reduce(:*)
          flat = @element.resolved.from_bulk(inp.array(@bulk, total))
          @dims.reverse[0...-1].each { |d| flat = flat.each_slice(d).to_a }
          flat
        else
          build(inp, 0)
        end
      end

      private

      def check(v, depth)
        return if v.is_a?(Array) && v.size == @dims[depth]

        Types.invalid("an Array of #{@dims[depth]} expected for a #{@name}, got #{v.inspect}")
      end

      def flatten(v, depth, flat)
        v = v.chars if v.is_a?(String) && @bulk == 0x10 && depth == @dims.size - 1
        check(v, depth)
        if depth == @dims.size - 1
          flat.concat(v)
        else
          v.each { |e| flatten(e, depth + 1, flat) }
        end
      end

      def each_element(v, depth, &blk)
        check(v, depth)
        if depth == @dims.size - 1
          v.each(&blk)
        else
          v.each { |e| each_element(e, depth + 1, &blk) }
        end
      end

      def build(inp, depth)
        if depth == @dims.size - 1
          Array.new(@dims[depth]) { @element.read(inp) }
        else
          Array.new(@dims[depth]) { build(inp, depth + 1) }
        end
      end
    end

    def self.array(element, dims)
      ArrayType.new(element, Array(dims))
    end

    # A typedef: its values are those of the related type.
    class AliasType < Type
      attr_reader :related

      def initialize(name, related)
        super()
        @name = name
        @related = related
        @native = Native.type_alias(name, related.native)
      end

      attr_reader :native

      def resolved
        @related.resolved
      end

      def write(out, v)
        @related.write(out, v)
      end

      def read(inp)
        @related.read(inp)
      end
    end

    def self.alias_type(name, related)
      AliasType.new(name, related)
    end

    # An enum: its values are the Integers of its enumerators.
    class EnumType < Type
      attr_reader :values, :native

      def initialize(name, bit_bound, extensibility, literals)
        super()
        @name = name
        @values = literals.map { |l| l[1] }.freeze
        @set = @values.to_h { |v| [v, true] }.freeze
        @native = Native.type_enum(name, bit_bound, extensibility, literals)
      end

      def write(out, v)
        Types.invalid("#{v.inspect} is not a value of the enum #{@name}") unless @set.key?(v)
        out.enum(v)
      end

      def read(inp)
        inp.enum
      end
    end

    def self.enum_type(name, bit_bound, extensibility, literals)
      EnumType.new(name, bit_bound, extensibility, literals)
    end

    # A bitmask: an Integer of its bits.
    class BitmaskType < Type
      attr_reader :bit_bound, :native

      def initialize(name, bit_bound, extensibility, flags)
        super()
        @name = name
        @bit_bound = bit_bound
        @native = Native.type_bitmask(name, bit_bound, extensibility, flags)
      end

      def write(out, v)
        unless v.is_a?(Integer) && !v.negative? && (v >> @bit_bound).zero?
          Types.invalid("#{v.inspect} does not fit the bitmask #{@name}")
        end
        out.bitmask(v)
      end

      def read(inp)
        inp.bitmask
      end
    end

    def self.bitmask_type(name, bit_bound, extensibility, flags)
      BitmaskType.new(name, bit_bound, extensibility, flags)
    end

    Member = ::Struct.new(:name, :id, :type, :flags, :ivar, :labels) do
      def optional?
        (flags & OPTIONAL) != 0
      end

      def key?
        (flags & KEY) != 0
      end
    end

    # A struct: the values are instances of its class, whose MEMBERS (the
    # Ruby names, in IDL order) are its instance variables.
    class StructType < Type
      attr_reader :klass, :members, :native

      def initialize(klass, name, extensibility, flags, members)
        super()
        @klass = klass
        @name = name
        ruby = klass.const_defined?(:MEMBERS) ? klass::MEMBERS : members.map(&:first)
        if ruby.size != members.size
          raise ArgumentError, "#{klass}: #{ruby.size} MEMBERS for #{members.size} members of #{name}"
        end

        @members = members.each_with_index.map do |(n, id, t, f, _), i|
          Member.new(n, id, t, f, :"@#{ruby[i]}", nil).freeze
        end.freeze
        @native = Native.type_struct(name, extensibility, flags,
                                     members.map { |n, id, t, f, h| [n, id, t.native, f, h] })
      end

      def write(out, v)
        Types.invalid("a #{@klass} expected, got nil") if v.nil?
        @members.each do |m|
          x = v.instance_variable_get(m.ivar)
          if m.optional?
            out.present(!x.nil?)
            m.type.write(out, x) unless x.nil?
          else
            Types.invalid("#{@name}: #{m.name} is nil") if x.nil?
            m.type.write(out, x)
          end
        end
      end

      def read(inp)
        v = @klass.allocate
        @members.each do |m|
          x = m.optional? && !inp.present ? nil : m.type.read(inp)
          v.instance_variable_set(m.ivar, x)
        end
        v
      end
    end

    def self.struct_type(klass, name, extensibility, flags, members)
      StructType.new(klass, name, extensibility, flags, members)
    end

    # A union: the values are instances of its class, with _disc, _value
    # and _set(disc, value).
    class UnionType < Type
      attr_reader :klass, :discriminator, :members, :native

      def initialize(klass, name, extensibility, flags, discriminator, members)
        super()
        @klass = klass
        @name = name
        @discriminator = discriminator
        d = discriminator.resolved
        @disc_kind = if d.is_a?(Primitive)
                       { 0x01 => :boolean, 0x10 => :char8, 0x11 => :char16 }.fetch(d.kind, :integer)
                     else
                       :integer
                     end
        @members = members.map do |n, id, t, labels, f, _|
          Member.new(n, id, t, f, nil, labels.dup.freeze).freeze
        end.freeze
        @by_label = {}
        @members.each { |m| m.labels.each { |l| @by_label[l] = m } }
        @default = @members.find { |m| (m.flags & DEFAULT_LABEL) != 0 }
        @native = Native.type_union(name, extensibility, flags, discriminator.native,
                                    members.map { |n, id, t, l, f, h| [n, id, t.native, l, f, h] })
      end

      # The Integer of a discriminator value.
      def label_of(disc)
        case @disc_kind
        when :boolean then disc ? 1 : 0
        when :char8, :char16 then disc.is_a?(String) ? disc.ord : disc
        else disc
        end
      end

      # The discriminator value of an Integer label.
      def disc_of(label)
        case @disc_kind
        when :boolean then label != 0
        when :char8 then label.chr(Encoding::UTF_8)
        else label
        end
      end

      # The member disc selects, or nil.
      def member_for(disc)
        return nil if disc.nil?

        @by_label.fetch(label_of(disc)) { @default }
      end

      # A discriminator value that no label has.
      def implicit_default
        @implicit_default ||= begin
          used = @by_label.keys
          d = @discriminator.resolved
          label = case @disc_kind
                  when :boolean then [0, 1].find { |x| !used.include?(x) }
                  when :char8 then (0..255).find { |x| !used.include?(x) }
                  else
                    if d.is_a?(EnumType)
                      d.values.find { |x| !used.include?(x) } || d.values.first
                    else
                      (0..).find { |x| !used.include?(x) }
                    end
                  end
          disc_of(label.nil? ? 0 : label)
        end
      end

      def write(out, v)
        Types.invalid("a #{@klass} expected, got nil") if v.nil?
        disc = v._disc
        Types.invalid("#{@name}: no discriminator") if disc.nil?
        if @disc_kind == :boolean
          Types.invalid("#{@name}: true or false expected, got #{disc.inspect}") unless [true, false].include?(disc)
        elsif !(disc.is_a?(Integer) || (disc.is_a?(String) && @disc_kind != :integer))
          Types.invalid("#{@name}: a discriminator expected, got #{disc.inspect}")
        end
        out.discriminator(label_of(disc))
        m = member_for(disc)
        return unless m

        x = v._value
        Types.invalid("#{@name}: #{m.name} is nil") if x.nil?
        m.type.write(out, x)
      end

      def read(inp)
        disc = disc_of(inp.discriminator)
        m = member_for(disc)
        value = m ? m.type.read(inp) : nil
        @klass.allocate._set(disc, value)
      end
    end

    def self.union_type(klass, name, extensibility, flags, discriminator, members)
      UnionType.new(klass, name, extensibility, flags, discriminator, members)
    end

    # The type of a class or module of rustorb-idl, or a Type.
    def self.of(type)
      return type if type.is_a?(Type)
      return type._dds_type if type.respond_to?(:_dds_type)

      raise ArgumentError, "#{type.inspect} is not a DDS type (generate it with rustorb-idl -bruby -Wbdds_only)"
    end

    # The values of v in a new sample stream.
    def self.out(type, v)
      out = Native::Out.new
      type.write(out, v)
      out
    end

    # The value in a sample stream, which is released.
    def self.read(type, inp)
      v = type.read(inp)
      inp.check
      v
    ensure
      inp.release
    end

    # v serialized (XCDR1, or XCDR2 with xcdr2), with its encapsulation
    # header: for tests and tools.
    def self.serialize(v, xcdr2: false, type: nil)
      t = of(type || v.class)
      Native.serialize(t.native, out(t, v), xcdr2)
    end

    # A value of type from its serialized form.
    def self.deserialize(type, data)
      t = of(type)
      read(t, Native.deserialize(t.native, data))
    end
  end
end
